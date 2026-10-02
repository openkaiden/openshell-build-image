// Copyright (C) 2026 Red Hat, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Nameservers for the build VM.
//!
//! libkrun's TSI network mode gives the guest no virtual NIC: its socket calls
//! are forwarded to the host, which makes the real connection. So there is no
//! DHCP to supply a `/etc/resolv.conf`, and a DNS query still travels to
//! whatever address the guest names — the host's resolver configuration is not
//! consulted, only its routing and firewall rules.
//!
//! A fixed public resolver is therefore wrong for many networks: it is blocked
//! where only the site's own resolver may be reached on port 53, and blind to
//! internal names, such as a registry mirror's. This discovers the host's
//! resolvers on every run instead, falling back to [`FALLBACK_NAMESERVER`] only
//! when none of them can work.
//!
//! Loopback resolvers are the ones that cannot work: `127.0.0.0/8` stays inside
//! the guest, so systemd-resolved's `127.0.0.53` or a VPN client's local stub
//! reaches nothing. [`is_reachable_from_vm`] rejects those.

use std::net::{IpAddr, Ipv4Addr};

/// Nameserver used when none of the host's own can be. A last resort: it only
/// works where outbound DNS is unrestricted and no internal names are needed.
pub const FALLBACK_NAMESERVER: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

/// Most nameservers a `resolv.conf` is read for — `MAXNS` in `<resolv.h>`.
/// Entries past this are ignored by the resolver, so there is no point
/// forwarding them.
const MAX_NAMESERVERS: usize = 3;

/// Resolver configuration read when the platform offers nothing better.
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// macOS's resolver configuration tool, by absolute path so the lookup does not
/// depend on the caller's `PATH`.
#[cfg(target_os = "macos")]
const SCUTIL: &str = "/usr/sbin/scutil";

/// Returns the nameservers a build VM should use on this host.
///
/// Discovers the host's own resolvers, drops any that cannot be reached from
/// inside the VM, and falls back to [`FALLBACK_NAMESERVER`] if that leaves
/// nothing. The result is never empty.
///
/// # Examples
///
/// ```
/// use vm_image_builder::dns;
///
/// let nameservers = dns::default_nameservers();
/// assert!(!nameservers.is_empty());
/// assert!(nameservers.iter().all(dns::is_reachable_from_vm));
/// ```
pub fn default_nameservers() -> Vec<IpAddr> {
    let discovered = host_nameservers();
    if discovered.is_empty() {
        log::debug!("no usable host nameserver found, falling back to {FALLBACK_NAMESERVER}");
        return vec![FALLBACK_NAMESERVER];
    }
    log::debug!(
        "using the host's nameservers in the VM: {}",
        join(&discovered)
    );
    discovered
}

/// Returns whether `addr` can serve DNS for a process inside the VM.
///
/// Rejects the addresses that mean something different in the guest than on the
/// host — loopback above all.
///
/// # Examples
///
/// ```
/// use std::net::IpAddr;
/// use vm_image_builder::dns::is_reachable_from_vm;
///
/// assert!(is_reachable_from_vm(&"192.168.1.1".parse::<IpAddr>().unwrap()));
/// // systemd-resolved's stub, like anything on loopback, would be the VM.
/// assert!(!is_reachable_from_vm(&"127.0.0.53".parse::<IpAddr>().unwrap()));
/// ```
pub fn is_reachable_from_vm(addr: &IpAddr) -> bool {
    if addr.is_loopback() || addr.is_unspecified() || addr.is_multicast() {
        return false;
    }
    match addr {
        IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_broadcast(),
        // fe80::/10 is only meaningful with a scope identifier naming a host
        // interface, which the guest does not have.
        IpAddr::V6(v6) => v6.segments()[0] & 0xffc0 != 0xfe80,
    }
}

/// Formats nameservers for a log line or an environment variable.
pub(crate) fn join(addrs: &[IpAddr]) -> String {
    addrs
        .iter()
        .map(IpAddr::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns the host's own resolvers, filtered and deduplicated. Empty when none
/// could be discovered or none are usable.
fn host_nameservers() -> Vec<IpAddr> {
    // macOS keeps its resolver configuration in SystemConfiguration;
    // /etc/resolv.conf is only a copy of the primary resolver, not always
    // current. Ask for the real thing first, and fall back for the hosts that
    // have no scutil — every one of which is a host no VM build runs on.
    #[cfg(target_os = "macos")]
    if let Some(output) = scutil_output() {
        let addrs = sanitize(parse_scutil(&output));
        if !addrs.is_empty() {
            return addrs;
        }
    }

    match std::fs::read_to_string(RESOLV_CONF) {
        Ok(text) => sanitize(parse_resolv_conf(&text)),
        Err(e) => {
            log::debug!("could not read {RESOLV_CONF}: {e}");
            Vec::new()
        }
    }
}

/// Runs `scutil --dns` and returns its output, or `None` if it could not be
/// run — a host without it simply falls through to [`RESOLV_CONF`].
#[cfg(target_os = "macos")]
fn scutil_output() -> Option<String> {
    let output = std::process::Command::new(SCUTIL)
        .arg("--dns")
        .output()
        .inspect_err(|e| log::debug!("could not run {SCUTIL}: {e}"))
        .ok()?;
    if !output.status.success() {
        log::debug!("{SCUTIL} --dns exited with {}", output.status);
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Extracts the addresses from the `nameserver[N] : ADDR` lines of
/// `scutil --dns` output, in the priority order it prints them. The same
/// address can appear under several resolvers; [`sanitize`] deduplicates.
#[cfg(target_os = "macos")]
fn parse_scutil(output: &str) -> Vec<IpAddr> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("nameserver[")?;
            let (_index, addr) = rest.split_once(" : ")?;
            addr.trim().parse().ok()
        })
        .collect()
}

/// Extracts the addresses from the `nameserver` lines of a `resolv.conf`.
fn parse_resolv_conf(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            // `#` and `;` both start a comment in resolv.conf(5).
            let line = line.split(['#', ';']).next().unwrap_or_default().trim();
            let rest = line.strip_prefix("nameserver")?;
            // Without this, a directive merely starting with those letters
            // would have its tail parsed as an address.
            if !rest.starts_with(char::is_whitespace) {
                return None;
            }
            rest.trim().parse().ok()
        })
        .collect()
}

/// Drops the addresses the guest cannot use and the duplicates, keeping the
/// first [`MAX_NAMESERVERS`] of what is left, in the order given.
fn sanitize(addrs: Vec<IpAddr>) -> Vec<IpAddr> {
    let mut kept: Vec<IpAddr> = Vec::new();
    for addr in addrs {
        if !is_reachable_from_vm(&addr) {
            log::debug!("ignoring host nameserver {addr}: unreachable from inside the VM");
            continue;
        }
        if !kept.contains(&addr) {
            kept.push(addr);
        }
        if kept.len() == MAX_NAMESERVERS {
            break;
        }
    }
    kept
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    // --- is_reachable_from_vm ---

    #[test]
    fn routable_addresses_are_reachable() {
        for addr in [
            "192.168.1.254",
            "10.0.0.1",
            "8.8.8.8",
            "1.1.1.1",
            "fd0f:ee:b0::1",
            "2606:4700:4700::1111",
        ] {
            assert!(is_reachable_from_vm(&ip(addr)), "{addr} should be usable");
        }
    }

    #[test]
    fn addresses_that_mean_something_else_in_the_guest_are_rejected() {
        for addr in [
            // The reason this module exists: a host-local stub resolver.
            "127.0.0.53",
            "127.0.0.1",
            "::1",
            "0.0.0.0",
            "::",
            "169.254.1.1",
            "fe80::1",
            "febf::1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(
                !is_reachable_from_vm(&ip(addr)),
                "{addr} should be rejected"
            );
        }
    }

    #[test]
    fn the_fallback_is_usable() {
        assert!(is_reachable_from_vm(&FALLBACK_NAMESERVER));
    }

    // --- parse_scutil ---
    //
    // Gated like the function itself: `scutil` is only consulted on macOS, so
    // off it the parser has no callers and clippy rejects it as dead code.

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_scutil_reads_every_resolver_in_order() {
        let output = "\
DNS configuration

resolver #1
  nameserver[0] : 192.168.1.254
  nameserver[1] : fd0f:ee:b0::1
  if_index : 5 (en0)
  flags    : Request A records, Request AAAA records

resolver #2
  domain   : local
  options  : mdns
  timeout  : 5

resolver #3
  nameserver[0] : 10.0.0.1
";
        assert_eq!(
            parse_scutil(output),
            vec![ip("192.168.1.254"), ip("fd0f:ee:b0::1"), ip("10.0.0.1")]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_scutil_ignores_everything_that_is_not_a_nameserver() {
        // `if_index` and `reach` are formatted exactly like a nameserver line.
        let output = "  if_index : 5 (en0)\n  reach : 0x00020002 (Reachable)\n  order : 300000\n";
        assert!(parse_scutil(output).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_scutil_handles_empty_output() {
        assert!(parse_scutil("").is_empty());
    }

    // --- parse_resolv_conf ---

    #[test]
    fn parse_resolv_conf_reads_nameserver_lines() {
        let text = "\
# generated by something
search example.com
nameserver 192.168.1.254
nameserver 8.8.8.8
options edns0
";
        assert_eq!(
            parse_resolv_conf(text),
            vec![ip("192.168.1.254"), ip("8.8.8.8")]
        );
    }

    #[test]
    fn parse_resolv_conf_skips_comments() {
        let text = "#nameserver 9.9.9.9\n;nameserver 9.9.9.10\nnameserver 8.8.8.8 # inline\n";
        assert_eq!(parse_resolv_conf(text), vec![ip("8.8.8.8")]);
    }

    #[test]
    fn parse_resolv_conf_requires_a_separator_after_the_directive() {
        // Without the whitespace check this would parse as 8.8.8.8.
        assert!(parse_resolv_conf("nameserverX 8.8.8.8\n").is_empty());
    }

    #[test]
    fn parse_resolv_conf_ignores_unparseable_addresses() {
        let text = "nameserver not-an-address\nnameserver 8.8.8.8\n";
        assert_eq!(parse_resolv_conf(text), vec![ip("8.8.8.8")]);
    }

    // --- sanitize ---

    #[test]
    fn sanitize_drops_unreachable_addresses() {
        let addrs = vec![ip("127.0.0.53"), ip("192.168.1.254"), ip("fe80::1")];
        assert_eq!(sanitize(addrs), vec![ip("192.168.1.254")]);
    }

    #[test]
    fn sanitize_deduplicates_while_keeping_order() {
        let addrs = vec![ip("8.8.8.8"), ip("1.1.1.1"), ip("8.8.8.8")];
        assert_eq!(sanitize(addrs), vec![ip("8.8.8.8"), ip("1.1.1.1")]);
    }

    #[test]
    fn sanitize_caps_at_the_resolver_limit() {
        let addrs = vec![
            ip("1.0.0.1"),
            ip("1.0.0.2"),
            ip("1.0.0.3"),
            ip("1.0.0.4"),
            ip("1.0.0.5"),
        ];
        let kept = sanitize(addrs);
        assert_eq!(kept.len(), MAX_NAMESERVERS);
        assert_eq!(kept[0], ip("1.0.0.1"));
    }

    #[test]
    fn sanitize_of_nothing_usable_is_empty() {
        assert!(sanitize(vec![ip("127.0.0.1"), ip("::1")]).is_empty());
    }

    // --- default_nameservers ---

    #[test]
    fn default_nameservers_is_never_empty_and_always_usable() {
        // The host running the tests may have any resolver configuration, or
        // none at all, so only the invariants callers rely on are asserted.
        let nameservers = default_nameservers();
        assert!(!nameservers.is_empty());
        assert!(nameservers.iter().all(is_reachable_from_vm));
        assert!(nameservers.len() <= MAX_NAMESERVERS);
    }

    // --- join ---

    #[test]
    fn join_separates_with_spaces() {
        assert_eq!(join(&[ip("8.8.8.8"), ip("fd00::1")]), "8.8.8.8 fd00::1");
        assert_eq!(join(&[]), "");
    }
}

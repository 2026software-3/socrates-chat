//! 判定登入請求的來源 IP（S-08.2 登入限流）。
//!
//! 預設只信任 TCP 連線的對端位址。只有對端落在 `TRUSTED_PROXIES` 的反向代理網段時，
//! 才讀 `X-Forwarded-For`，且取「由右往左第一個不屬於受信任代理」的位址：代理自己追加的
//! 項目在最右邊，左邊的項目可能是用戶端偽造的。

use std::{
    net::{IpAddr, Ipv6Addr},
    str::FromStr,
};

/// 一個 CIDR 網段，例如 `127.0.0.0/8`；沒有 `/` 時視為單一位址。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl FromStr for Cidr {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (addr, prefix) = match s.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (s, None),
        };
        let addr: IpAddr = addr.parse().map_err(|_| format!("invalid address: {s}"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p
                .parse::<u8>()
                .map_err(|_| format!("invalid prefix: {s}"))?,
            None => max,
        };
        if prefix > max {
            return Err(format!("prefix too long: {s}"));
        }
        Ok(Self { addr, prefix })
    }
}

impl Cidr {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, normalize(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                prefix_matches(u32::from(net).into(), u32::from(ip).into(), self.prefix, 32)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                prefix_matches(u128::from(net), u128::from(ip), self.prefix, 128)
            }
            _ => false,
        }
    }
}

/// 兩個位址的前 `prefix` 個位元（共 `bits` 位元）是否相同。
fn prefix_matches(a: u128, b: u128, prefix: u8, bits: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let shift = u32::from(bits - prefix);
    (a >> shift) == (b >> shift)
}

/// IPv4 mapped IPv6（`::ffff:a.b.c.d`）還原成 IPv4。
fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

/// 以逗號分隔的 CIDR 清單；空字串得到空清單。
pub fn parse_cidrs(raw: &str) -> Result<Vec<Cidr>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect()
}

/// 來源 IP：`peer` 是 TCP 對端；`forwarded_for` 是 `X-Forwarded-For` 的原始值。
/// 沒有對端資訊時回 `None`。
pub fn resolve(
    peer: Option<IpAddr>,
    forwarded_for: Option<&str>,
    trusted: &[Cidr],
) -> Option<IpAddr> {
    let peer = peer?;
    let is_trusted = |ip: IpAddr| trusted.iter().any(|c| c.contains(ip));
    if !is_trusted(peer) {
        return Some(peer);
    }
    let Some(header) = forwarded_for else {
        return Some(peer);
    };
    // 由右往左：代理追加的項目在右邊。遇到無法解析的項目就不信任整個標頭
    for entry in header.rsplit(',') {
        match entry.trim().parse::<IpAddr>() {
            Ok(ip) if is_trusted(ip) => continue,
            Ok(ip) => return Some(ip),
            Err(_) => return Some(peer),
        }
    }
    Some(peer)
}

/// 一個請求的限流鍵（`resolve` 後再 `rate_limit_key`）；沒有連線資訊時為空字串。
pub fn client_key(peer: Option<IpAddr>, forwarded_for: Option<&str>, trusted: &[Cidr]) -> String {
    resolve(peer, forwarded_for, trusted)
        .map(rate_limit_key)
        .unwrap_or_default()
}

/// 限流用的鍵：IPv4 mapped IPv6 還原成 IPv4，IPv6 只留前 64 位元（同一個 /64 視為同一來源）。
pub fn rate_limit_key(ip: IpAddr) -> String {
    match normalize(ip) {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let masked = u128::from(v6) & (u128::MAX << 64);
            format!("{}/64", Ipv6Addr::from(masked))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn trusted(raw: &str) -> Vec<Cidr> {
        parse_cidrs(raw).unwrap()
    }

    #[test]
    fn cidr_matches_v4_and_v6_ranges() {
        let c: Cidr = "172.16.0.0/12".parse().unwrap();
        assert!(c.contains(ip("172.17.0.1")));
        assert!(!c.contains(ip("172.32.0.1")));
        assert!(!c.contains(ip("::1")));
        let single: Cidr = "10.0.0.5".parse().unwrap();
        assert!(single.contains(ip("10.0.0.5")));
        assert!(!single.contains(ip("10.0.0.6")));
        let v6: Cidr = "fd00::/8".parse().unwrap();
        assert!(v6.contains(ip("fd12::1")));
        assert!(!v6.contains(ip("fe80::1")));
        let all: Cidr = "0.0.0.0/0".parse().unwrap();
        assert!(all.contains(ip("8.8.8.8")));
    }

    #[test]
    fn invalid_cidrs_are_rejected() {
        for bad in ["nope", "10.0.0.0/33", "::/129", "10.0.0.0/x", "/8"] {
            assert!(bad.parse::<Cidr>().is_err(), "{bad}");
        }
        assert!(parse_cidrs("127.0.0.1, bogus").is_err());
        assert_eq!(parse_cidrs("").unwrap(), vec![]);
        assert_eq!(parse_cidrs(" 127.0.0.0/8 , ::1 ").unwrap().len(), 2);
    }

    #[test]
    fn header_is_ignored_unless_peer_is_a_trusted_proxy() {
        // 沒設定受信任代理：偽造的標頭完全無效
        assert_eq!(
            resolve(Some(ip("198.51.100.7")), Some("6.6.6.6"), &[]),
            Some(ip("198.51.100.7"))
        );
        // 有設定，但對端不是代理（直連者自己填標頭）
        assert_eq!(
            resolve(
                Some(ip("198.51.100.7")),
                Some("6.6.6.6"),
                &trusted("127.0.0.0/8")
            ),
            Some(ip("198.51.100.7"))
        );
    }

    #[test]
    fn trusted_proxy_header_gives_the_client_address() {
        let t = trusted("127.0.0.0/8");
        assert_eq!(
            resolve(Some(ip("127.0.0.1")), Some("203.0.113.9"), &t),
            Some(ip("203.0.113.9"))
        );
        // 沒有標頭：退回對端
        assert_eq!(
            resolve(Some(ip("127.0.0.1")), None, &t),
            Some(ip("127.0.0.1"))
        );
    }

    #[test]
    fn rightmost_untrusted_entry_wins_so_forged_left_entries_are_ignored() {
        let t = trusted("127.0.0.0/8, 10.0.0.0/8");
        assert_eq!(
            resolve(Some(ip("127.0.0.1")), Some("6.6.6.6, 203.0.113.9"), &t),
            Some(ip("203.0.113.9"))
        );
        // 多層代理：右邊受信任的跳過
        assert_eq!(
            resolve(
                Some(ip("127.0.0.1")),
                Some("6.6.6.6, 203.0.113.9, 10.1.2.3"),
                &t
            ),
            Some(ip("203.0.113.9"))
        );
    }

    #[test]
    fn malformed_header_falls_back_to_the_peer() {
        let t = trusted("127.0.0.0/8");
        for bad in ["", "not-an-ip", "203.0.113.9, garbage"] {
            assert_eq!(
                resolve(Some(ip("127.0.0.1")), Some(bad), &t),
                Some(ip("127.0.0.1")),
                "{bad}"
            );
        }
    }

    #[test]
    fn no_peer_means_no_ip() {
        assert_eq!(
            resolve(None, Some("203.0.113.9"), &trusted("127.0.0.0/8")),
            None
        );
    }

    #[test]
    fn rate_limit_key_normalises_addresses() {
        assert_eq!(rate_limit_key(ip("203.0.113.9")), "203.0.113.9");
        assert_eq!(rate_limit_key(ip("::ffff:203.0.113.9")), "203.0.113.9");
        // 同一個 /64 是同一個來源，不同 /64 不同
        assert_eq!(
            rate_limit_key(ip("2001:db8:1:2:aaaa::1")),
            rate_limit_key(ip("2001:db8:1:2:bbbb::2"))
        );
        assert_ne!(
            rate_limit_key(ip("2001:db8:1:2::1")),
            rate_limit_key(ip("2001:db8:1:3::1"))
        );
    }
}

// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! Turning an IPv6 string into 16 bytes, nothing else.

/// An IPv6 address as 16 bytes, `::` and all.
///
/// Hand-rolled because `Ipv6Addr::from_str` would do it — but only after the
/// zone index is gone, and it rejects the whole address rather than the suffix,
/// which is a worse error to debug than this is to write.
pub(super) fn expand_ipv6(addr: &str) -> Option<[u8; 16]> {
    let (head, tail) = match addr.split_once("::") {
        Some((h, t)) => (h, Some(t)),
        None => (addr, None),
    };
    let parse_groups = |s: &str| -> Option<Vec<u16>> {
        if s.is_empty() {
            return Some(Vec::new());
        }
        s.split(':').map(|g| u16::from_str_radix(g, 16).ok()).collect()
    };

    let head = parse_groups(head)?;
    let tail = match tail {
        Some(t) => parse_groups(t)?,
        None => Vec::new(),
    };
    if head.len() + tail.len() > 8 {
        return None;
    }
    // Without `::` the address must be complete; with it, the gap is zeros.
    if addr.find("::").is_none() && head.len() != 8 {
        return None;
    }

    let mut groups = [0u16; 8];
    groups[..head.len()].copy_from_slice(&head);
    let start = 8 - tail.len();
    groups[start..].copy_from_slice(&tail);

    let mut out = [0u8; 16];
    for (i, g) in groups.iter().enumerate() {
        out[i * 2] = (g >> 8) as u8;
        out[i * 2 + 1] = (g & 0xFF) as u8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_every_shape_of_ipv6() {
        assert_eq!(expand_ipv6("::").unwrap(), [0u8; 16]);
        assert_eq!(expand_ipv6("fe80::1").unwrap()[0], 0xFE);
        assert_eq!(*expand_ipv6("fe80::1").unwrap().last().unwrap(), 1);
        assert!(expand_ipv6("fe80:0:0:0:0:0:0").is_none(), "short without ::");
        assert!(expand_ipv6("gg::1").is_none());
    }
}

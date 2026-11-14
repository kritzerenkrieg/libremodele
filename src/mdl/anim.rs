//! GoldSrc animation channel data (`mstudioanimvalue_t`): run-length coded
//! shorts. Each run is a header (`valid`, `total`: bytes) followed by `valid`
//! values. It covers `total` frames, and frames past `valid` repeat the last
//! value.

use anyhow::{Result, bail};

use super::bytes::Cur;

/// The per-frame values of one channel starting at `at`.
pub fn decode(d: &[u8], at: usize, frames: usize) -> Result<Vec<i16>> {
    let mut c = Cur::new(d, at);
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        let valid = c.u8()? as usize;
        let total = c.u8()? as usize;
        if valid == 0 || total == 0 || valid > total {
            bail!(
                "bad animation run at {}: valid {valid}, total {total}",
                c.pos - 2
            );
        }
        let mut vals = Vec::with_capacity(valid);
        for _ in 0..valid {
            vals.push(c.i16()?);
        }
        for i in 0..total {
            out.push(vals[i.min(valid - 1)]);
        }
    }
    out.truncate(frames);
    Ok(out)
}

/// The run-length bytes for `vals`, the inverse of `decode`.
pub fn encode(vals: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    for run in vals.chunks(255) {
        let last = run[run.len() - 1];
        // Values up to and including the first of the repeating tail.
        let valid = run.iter().rposition(|&v| v != last).map_or(1, |j| j + 2);
        out.push(valid as u8);
        out.push(run.len() as u8);
        for v in &run[..valid] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(vals: &[i16]) {
        let bytes = encode(vals);
        assert_eq!(decode(&bytes, 0, vals.len()).unwrap(), vals);
    }

    #[test]
    fn round_trips() {
        round(&[5]);
        round(&[1, 2, 3, 3, 3]);
        round(&[7, 7, 7, 7]);
        round(&[1, 2, 1, 2]);
        let long: Vec<i16> = (0..700).map(|i| (i / 3) as i16).collect();
        round(&long);
        round(&[-3; 600]);
    }

    #[test]
    fn repeats_tail() {
        assert_eq!(encode(&[4, 9, 9, 9]), vec![2, 4, 4, 0, 9, 0]);
    }
}

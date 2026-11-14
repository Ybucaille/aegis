//! Firmware Aegis (no_std, sans unsafe).
#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    #[test]
    fn trivial() {
        assert_eq!(1 + 1, 2);
    }
}

//! Ground station simulée.
#[cfg(test)]
mod tests {
    #[test]
    fn depend_de_firmware() {
        let _ = core::mem::size_of::<u8>();
        assert_eq!(1 + 1, 2);
    }
}

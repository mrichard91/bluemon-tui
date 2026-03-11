/// Google Fast Pair model ID lookup table.
///
/// The model ID is a 3-byte value transmitted in the manufacturer data
/// for Google (company ID 0x00E0). Maps known model IDs to device names.
const FAST_PAIR_MODELS: &[(u32, &str)] = &[
    // Google Pixel Buds
    (0x000047, "Pixel Buds"),
    (0x00006B, "Pixel Buds A-Series"),
    (0x0000C0, "Pixel Buds Pro"),
    (0x0000F0, "Pixel Buds Pro 2"),
    // Samsung Galaxy Buds
    (0x0000A8, "Galaxy Buds Live"),
    (0x000108, "Galaxy Buds Pro"),
    (0x000180, "Galaxy Buds2"),
    (0x000201, "Galaxy Buds2 Pro"),
    (0x0002D0, "Galaxy Buds FE"),
    (0x000350, "Galaxy Buds3"),
    (0x000351, "Galaxy Buds3 Pro"),
    // Sony
    (0x0000D8, "Sony WF-1000XM4"),
    (0x000228, "Sony WF-1000XM5"),
    (0x0000C8, "Sony WH-1000XM4"),
    (0x000238, "Sony WH-1000XM5"),
    (0x0000E8, "Sony LinkBuds"),
    (0x000178, "Sony LinkBuds S"),
    // JBL
    (0x00009C, "JBL Live Pro+"),
    (0x0000B8, "JBL Reflect Flow Pro"),
    (0x0000F8, "JBL Tune 230NC"),
    (0x000158, "JBL Live Pro 2"),
    (0x0001A0, "JBL Tour Pro 2"),
    // Beats
    (0x000098, "Beats Fit Pro"),
    (0x0001C0, "Beats Studio Buds+"),
    // Nothing
    (0x000120, "Nothing Ear (1)"),
    (0x0001F0, "Nothing Ear (2)"),
    (0x000220, "Nothing Ear (stick)"),
    // Bose
    (0x0000A0, "Bose QC Earbuds"),
    (0x000190, "Bose QC Earbuds II"),
    (0x0000D0, "Bose NC 700"),
    (0x000250, "Bose QC Ultra Earbuds"),
    // Others
    (0x0001B0, "OnePlus Buds Pro 2"),
    (0x0001D0, "Jabra Elite 85t"),
    (0x000168, "Anker Soundcore Liberty 4"),
    (0x0000E0, "Sennheiser Momentum TW3"),
    (0x000280, "Xiaomi Buds 4 Pro"),
];

/// Look up a device name from Google Fast Pair manufacturer data.
///
/// The manufacturer data for company 0x00E0 contains the model ID
/// in its first 3 bytes (big-endian 24-bit value).
pub fn lookup_model(data: &[u8]) -> Option<String> {
    if data.len() < 3 {
        return None;
    }
    let model_id = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);

    FAST_PAIR_MODELS
        .iter()
        .find(|(id, _)| *id == model_id)
        .map(|(_, name)| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_pixel_buds() {
        let data = [0x00, 0x00, 0x47]; // model ID 0x000047
        assert_eq!(lookup_model(&data), Some("Pixel Buds".to_string()));
    }

    #[test]
    fn lookup_galaxy_buds_pro() {
        let data = [0x00, 0x01, 0x08]; // model ID 0x000108
        assert_eq!(lookup_model(&data), Some("Galaxy Buds Pro".to_string()));
    }

    #[test]
    fn lookup_unknown_model() {
        let data = [0xFF, 0xFF, 0xFF];
        assert_eq!(lookup_model(&data), None);
    }

    #[test]
    fn lookup_short_data() {
        assert_eq!(lookup_model(&[0x00, 0x00]), None);
        assert_eq!(lookup_model(&[0x00]), None);
        assert_eq!(lookup_model(&[]), None);
    }

    #[test]
    fn lookup_extra_bytes_ignored() {
        // Extra bytes after the 3-byte model ID should be ignored
        let data = [0x00, 0x00, 0x47, 0xFF, 0xFF];
        assert_eq!(lookup_model(&data), Some("Pixel Buds".to_string()));
    }
}

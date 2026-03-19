//! MAC OUI vendor lookup using the IEEE database (via `mac_oui` crate).

use mac_oui::Oui;
use std::sync::OnceLock;

static OUI_DB: OnceLock<Oui> = OnceLock::new();

fn db() -> &'static Oui {
    OUI_DB.get_or_init(|| Oui::default().expect("failed to load OUI database"))
}

/// Look up the vendor/manufacturer for a MAC address using the IEEE OUI database.
/// Returns None for invalid MACs or unknown OUIs.
pub fn lookup_vendor(mac: &str) -> Option<String> {
    let result = db().lookup_by_mac(mac).ok()??;
    Some(result.company_name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_oui() {
        // 00:17:F2 is a well-known Apple OUI prefix
        let vendor = lookup_vendor("00:17:F2:00:11:22");
        assert!(vendor.is_some(), "Expected a known vendor for Apple OUI");
        let name = vendor.unwrap();
        assert!(
            name.to_lowercase().contains("apple"),
            "Expected Apple, got: {name}"
        );
    }

    #[test]
    fn lookup_unknown_oui() {
        // FF:FF:FF is not a valid OUI
        let vendor = lookup_vendor("FF:FF:FF:00:00:00");
        assert!(vendor.is_none());
    }

    #[test]
    fn lookup_invalid_mac() {
        assert!(lookup_vendor("").is_none());
        assert!(lookup_vendor("not-a-mac").is_none());
    }
}

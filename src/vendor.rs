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

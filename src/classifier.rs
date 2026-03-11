use ratatui::style::Color;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Phone,
    Tablet,
    Laptop,
    Computer,
    Watch,
    Audio,
    Speaker,
    Tv,
    Vehicle,
    SmartHome,
    Wearable,
    Gaming,
    Camera,
    Printer,
    Network,
    Unknown,
}

impl DeviceType {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Phone => "[PHN]",
            Self::Tablet => "[TAB]",
            Self::Laptop => "[LAP]",
            Self::Computer => "[PC]",
            Self::Watch => "[WCH]",
            Self::Audio => "[AUD]",
            Self::Speaker => "[SPK]",
            Self::Tv => "[TV]",
            Self::Vehicle => "[CAR]",
            Self::SmartHome => "[IOT]",
            Self::Wearable => "[WRB]",
            Self::Gaming => "[GAM]",
            Self::Camera => "[CAM]",
            Self::Printer => "[PRT]",
            Self::Network => "[NET]",
            Self::Unknown => "[---]",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Phone => "Phone",
            Self::Tablet => "Tablet",
            Self::Laptop => "Laptop",
            Self::Computer => "Computer",
            Self::Watch => "Watch",
            Self::Audio => "Audio",
            Self::Speaker => "Speaker",
            Self::Tv => "TV/Display",
            Self::Vehicle => "Vehicle",
            Self::SmartHome => "Smart Home",
            Self::Wearable => "Wearable",
            Self::Gaming => "Gaming",
            Self::Camera => "Camera",
            Self::Printer => "Printer",
            Self::Network => "Network",
            Self::Unknown => "Unknown",
        }
    }

    pub fn to_db(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Tablet => "tablet",
            Self::Laptop => "laptop",
            Self::Computer => "computer",
            Self::Watch => "watch",
            Self::Audio => "audio",
            Self::Speaker => "speaker",
            Self::Tv => "tv",
            Self::Vehicle => "vehicle",
            Self::SmartHome => "smart",
            Self::Wearable => "wearable",
            Self::Gaming => "gaming",
            Self::Camera => "camera",
            Self::Printer => "printer",
            Self::Network => "network",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "phone" => Self::Phone,
            "tablet" => Self::Tablet,
            "laptop" => Self::Laptop,
            "computer" => Self::Computer,
            "watch" => Self::Watch,
            "audio" => Self::Audio,
            "speaker" => Self::Speaker,
            "tv" => Self::Tv,
            "vehicle" => Self::Vehicle,
            "smart" => Self::SmartHome,
            "wearable" => Self::Wearable,
            "gaming" => Self::Gaming,
            "camera" => Self::Camera,
            "printer" => Self::Printer,
            "network" => Self::Network,
            _ => Self::Unknown,
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Phone => Color::Cyan,
            Self::Tablet => Color::LightCyan,
            Self::Laptop => Color::Blue,
            Self::Computer => Color::LightBlue,
            Self::Watch => Color::Magenta,
            Self::Audio => Color::Yellow,
            Self::Speaker => Color::LightYellow,
            Self::Tv => Color::Green,
            Self::Vehicle => Color::LightRed,
            Self::SmartHome => Color::LightGreen,
            Self::Wearable => Color::LightMagenta,
            Self::Gaming => Color::Red,
            Self::Camera => Color::White,
            Self::Printer => Color::Gray,
            Self::Network => Color::DarkGray,
            Self::Unknown => Color::DarkGray,
        }
    }
}

// ---------------------------------------------------------------------------
// Bluetooth SIG company identifiers → (name, default device type)
//
// These are the most commonly seen BLE advertisers. The company ID is a u16
// from the manufacturer_data field in BLE advertisements.
// Source: Bluetooth SIG Assigned Numbers
// ---------------------------------------------------------------------------
const COMPANY_IDS: &[(u16, &str, DeviceType)] = &[
    // Apple — sub-typed further by parse_apple_manufacturer_data()
    (0x004C, "Apple", DeviceType::Phone),
    // Mobile / Phone manufacturers
    (0x0075, "Samsung", DeviceType::Phone),
    (0x00E0, "Google", DeviceType::Phone),
    (0x038F, "Xiaomi", DeviceType::Phone),
    (0x0010, "Motorola", DeviceType::Phone),
    (0x0101, "Huawei", DeviceType::Phone),
    (0x00DA, "OPPO", DeviceType::Phone),
    (0x02E5, "OnePlus", DeviceType::Phone),
    (0x0652, "Realme", DeviceType::Phone),
    (0x001A, "Nokia", DeviceType::Phone),
    (0x065A, "Nothing", DeviceType::Phone),
    (0x0131, "LG", DeviceType::Phone),
    (0x028E, "ZTE", DeviceType::Phone),
    // Computers
    (0x0006, "Microsoft", DeviceType::Computer),
    (0x0002, "Intel", DeviceType::Computer),
    (0x000A, "Qualcomm", DeviceType::Computer),
    (0x030B, "Lenovo", DeviceType::Laptop),
    (0x0209, "Dell", DeviceType::Laptop),
    // Audio
    (0x009E, "Bose", DeviceType::Audio),
    (0x012D, "Sony", DeviceType::Audio),
    (0x0087, "Jabra", DeviceType::Audio),
    (0x000F, "Broadcom", DeviceType::Audio),
    (0x0056, "Harman", DeviceType::Speaker),
    (0x057B, "JBL", DeviceType::Speaker),
    (0x02B6, "Sennheiser", DeviceType::Audio),
    (0x0106, "Plantronics", DeviceType::Audio),
    (0x04DA, "Skullcandy", DeviceType::Audio),
    (0x0397, "Sonos", DeviceType::Speaker),
    (0x0172, "Beats", DeviceType::Audio),
    (0x0310, "Bang & Olufsen", DeviceType::Speaker),
    // Watches / Wearables
    (0x0100, "Garmin", DeviceType::Watch),
    (0x02FF, "Fitbit", DeviceType::Watch),
    (0x018B, "Polar", DeviceType::Watch),
    (0x0154, "Suunto", DeviceType::Watch),
    (0x065B, "Whoop", DeviceType::Wearable),
    (0x0486, "Oura", DeviceType::Wearable),
    // Smart Home / IoT
    (0x0171, "Amazon", DeviceType::SmartHome),
    (0x0822, "Tile", DeviceType::SmartHome),
    (0x0011, "Philips", DeviceType::SmartHome),
    (0x028A, "IKEA", DeviceType::SmartHome),
    (0x0362, "SwitchBot", DeviceType::SmartHome),
    (0x060A, "Govee", DeviceType::SmartHome),
    (0x0047, "Ecobee", DeviceType::SmartHome),
    // TV / Streaming
    (0x0488, "Roku", DeviceType::Tv),
    // Gaming
    (0x0057, "Nintendo", DeviceType::Gaming),
    (0x02A5, "Valve", DeviceType::Gaming),
    (0x01A2, "Razer", DeviceType::Gaming),
    (0x0207, "SteelSeries", DeviceType::Gaming),
    (0x0046, "Logitech", DeviceType::Gaming),
    // Camera
    (0x0128, "GoPro", DeviceType::Camera),
    (0x018D, "DJI", DeviceType::Camera),
    // Network / Chipsets (default Unknown — used for vendor name only)
    (0x000D, "Texas Instruments", DeviceType::Unknown),
    (0x0059, "Nordic Semi", DeviceType::Unknown),
    (0x005D, "Realtek", DeviceType::Unknown),
    (0x004E, "MediaTek", DeviceType::Unknown),
    (0x0031, "Cypress", DeviceType::Unknown),
    (0x03DA, "Espressif", DeviceType::SmartHome),
    (0x0135, "Dialog Semi", DeviceType::Unknown),
];

/// Look up company name from a BLE manufacturer data company ID.
pub fn company_name(id: u16) -> Option<&'static str> {
    COMPANY_IDS
        .iter()
        .find(|(cid, _, _)| *cid == id)
        .map(|(_, name, _)| *name)
}

/// Look up default device type from manufacturer data company ID.
fn company_device_type(id: u16) -> Option<DeviceType> {
    COMPANY_IDS
        .iter()
        .find(|(cid, _, _)| *cid == id)
        .map(|(_, _, dt)| *dt)
        .filter(|dt| *dt != DeviceType::Unknown)
}

/// Get the best company name from all manufacturer data entries.
/// Returns the first recognized company, or the first company ID as hex.
pub fn best_company_name(manufacturer_data: &HashMap<u16, Vec<u8>>) -> Option<String> {
    if manufacturer_data.is_empty() {
        return None;
    }
    // Try recognized companies first
    for &id in manufacturer_data.keys() {
        if let Some(name) = company_name(id) {
            return Some(name.to_string());
        }
    }
    // Fall back to hex company ID
    let id = manufacturer_data.keys().next()?;
    Some(format!("BT#{:04X}", id))
}

// ---------------------------------------------------------------------------
// Apple manufacturer data parsing (company ID 0x004C)
//
// Apple Continuity protocol uses type bytes to identify data purpose:
// 0x02 = iBeacon, 0x07/0x12 = AirPods proximity, 0x09 = AirPlay,
// 0x0C = Handoff, 0x10 = Nearby Info, 0x19 = FindMy/AirTag
// ---------------------------------------------------------------------------
fn classify_apple_mfr_data(data: &[u8]) -> DeviceType {
    if data.is_empty() {
        return DeviceType::Phone;
    }
    match data[0] {
        0x02 => DeviceType::SmartHome,  // iBeacon
        0x07 | 0x12 => DeviceType::Audio, // AirPods/Beats proximity pairing
        0x09 => DeviceType::Speaker,    // AirPlay target
        0x0C => DeviceType::Computer,   // Handoff (Mac/iPad)
        0x19 => DeviceType::SmartHome,  // FindMy / AirTag
        _ => DeviceType::Phone,         // Default Apple = phone
    }
}

/// Classify a device by its BLE manufacturer data.
fn classify_by_manufacturer_data(manufacturer_data: &HashMap<u16, Vec<u8>>) -> Option<DeviceType> {
    // Apple gets special sub-type parsing
    if let Some(data) = manufacturer_data.get(&0x004C) {
        return Some(classify_apple_mfr_data(data));
    }
    // For other companies, use the default device type from the table
    for &id in manufacturer_data.keys() {
        if let Some(dt) = company_device_type(id) {
            return Some(dt);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Classic Bluetooth device class parsing
//
// The device class is a 24-bit field. Major class is bits 12-8.
// Source: Bluetooth SIG Assigned Numbers, Section 1.4
// ---------------------------------------------------------------------------
fn classify_by_device_class(device_class: u32) -> Option<DeviceType> {
    let major = (device_class >> 8) & 0x1F;
    match major {
        1 => Some(DeviceType::Computer),
        2 => Some(DeviceType::Phone),
        3 => Some(DeviceType::Network),
        4 => Some(DeviceType::Audio),
        5 => Some(DeviceType::Gaming),  // Peripheral (HID)
        6 => Some(DeviceType::Printer), // Imaging
        7 => Some(DeviceType::Wearable),
        9 => Some(DeviceType::Wearable), // Health
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Service UUID patterns
// ---------------------------------------------------------------------------
const SERVICE_UUID_PATTERNS: &[(&str, DeviceType)] = &[
    // Wearables / Fitness
    ("0000180d", DeviceType::Wearable),  // Heart Rate
    ("0000181c", DeviceType::Wearable),  // User Data
    ("00001814", DeviceType::Wearable),  // Running Speed
    ("00001816", DeviceType::Wearable),  // Cycling Speed
    ("00001818", DeviceType::Wearable),  // Cycling Power
    ("0000181b", DeviceType::Wearable),  // Body Composition
    ("0000181d", DeviceType::Wearable),  // Weight Scale
    // Health
    ("00001810", DeviceType::Wearable),  // Blood Pressure
    ("00001808", DeviceType::Wearable),  // Glucose
    ("00001809", DeviceType::Wearable),  // Health Thermometer
    // Audio
    ("0000110b", DeviceType::Audio),     // A2DP Sink
    ("0000110a", DeviceType::Audio),     // A2DP Source
    ("0000111e", DeviceType::Audio),     // Handsfree
    ("0000111f", DeviceType::Audio),     // Handsfree AG
    ("00001108", DeviceType::Audio),     // Headset
    ("0000110d", DeviceType::Audio),     // Advanced Audio
    ("00001203", DeviceType::Audio),     // Generic Audio
    ("0000184e", DeviceType::Audio),     // Audio Stream Control
    ("0000184f", DeviceType::Audio),     // Broadcast Audio
    ("00001850", DeviceType::Audio),     // Published Audio
    ("00001853", DeviceType::Audio),     // Common Audio
    // Gaming / HID
    ("00001812", DeviceType::Gaming),    // HID
    ("00001124", DeviceType::Gaming),    // HID legacy
    // Apple
    ("d0611e78", DeviceType::Phone),     // Apple Continuity
    ("7905f431", DeviceType::Phone),     // Apple Notification Center
    ("89d3502b", DeviceType::Phone),     // Apple Media Service
    ("0000fd6f", DeviceType::Phone),     // Apple Continuity short
    // Google/Android
    ("0000fe9f", DeviceType::Phone),     // Google Fast Pair
    ("0000fe2c", DeviceType::Phone),     // Google Nearby
    // Smart Home / IoT
    ("0000181a", DeviceType::SmartHome), // Environmental Sensing
    ("0000fef5", DeviceType::SmartHome), // Philips Hue
    ("0000fee7", DeviceType::SmartHome), // Tencent IoT
    ("0000feaa", DeviceType::SmartHome), // Eddystone
    ("0000feab", DeviceType::SmartHome), // Nokia beacons
    // Trackers
    ("0000feed", DeviceType::SmartHome), // Tile
    ("0000febe", DeviceType::SmartHome), // Bose
    ("0000feec", DeviceType::SmartHome), // Tile
    // Location
    ("00001819", DeviceType::Wearable),  // Location & Navigation
    // Watches
    ("cba20d00", DeviceType::Watch),     // SwitchBot
    ("0000fee0", DeviceType::Watch),     // Xiaomi Mi Band
    ("0000feea", DeviceType::Watch),     // Swirl Networks
    // Printers
    ("00001118", DeviceType::Printer),   // Direct Printing
    ("00001119", DeviceType::Printer),   // Reference Printing
    // Camera
    ("00001822", DeviceType::Camera),    // Camera Profile
    // Vehicles
    ("6f65732a", DeviceType::Vehicle),   // Rivian BLE
];

/// Name substring patterns → DeviceType (checked case-insensitively)
const NAME_PATTERNS: &[(&[&str], DeviceType)] = &[
    (&["iphone", "android", "pixel", "galaxy s", "galaxy z"], DeviceType::Phone),
    (&["ipad", "tab", "tablet"], DeviceType::Tablet),
    (&["macbook", "thinkpad", "xps", "laptop"], DeviceType::Laptop),
    (&["imac", "mac mini", "mac pro", "desktop"], DeviceType::Computer),
    (&["watch", "band", "mi band"], DeviceType::Watch),
    (&["airpod", "buds", "earbuds", "headphone"], DeviceType::Audio),
    (&["homepod", "echo", "speaker"], DeviceType::Speaker),
    (&["tv", "roku", "firestick", "chromecast"], DeviceType::Tv),
    (&["car", "vehicle", "model 3", "model y", "model s", "rivn"], DeviceType::Vehicle),
];

/// Vendor substring patterns → DeviceType (checked case-insensitively)
const VENDOR_PATTERNS: &[(&str, DeviceType)] = &[
    // Phones
    ("apple", DeviceType::Phone),
    ("samsung electronics", DeviceType::Phone),
    ("xiaomi", DeviceType::Phone),
    ("huawei", DeviceType::Phone),
    ("oneplus", DeviceType::Phone),
    ("oppo", DeviceType::Phone),
    ("vivo", DeviceType::Phone),
    ("realme", DeviceType::Phone),
    ("motorola", DeviceType::Phone),
    ("nokia", DeviceType::Phone),
    ("lg electronics", DeviceType::Phone),
    ("zte", DeviceType::Phone),
    ("google", DeviceType::Phone),
    ("fairphone", DeviceType::Phone),
    ("nothing", DeviceType::Phone),
    // Laptops / Computers
    ("dell", DeviceType::Laptop),
    ("lenovo", DeviceType::Laptop),
    ("hewlett packard", DeviceType::Laptop),
    ("hp inc", DeviceType::Laptop),
    ("asus", DeviceType::Laptop),
    ("acer", DeviceType::Laptop),
    ("microsoft", DeviceType::Computer),
    ("intel corporate", DeviceType::Computer),
    ("gigabyte", DeviceType::Computer),
    ("msi", DeviceType::Computer),
    // Audio
    ("bose", DeviceType::Audio),
    ("sony", DeviceType::Audio),
    ("sennheiser", DeviceType::Audio),
    ("jabra", DeviceType::Audio),
    ("beats", DeviceType::Audio),
    ("jbl", DeviceType::Speaker),
    ("harman", DeviceType::Speaker),
    ("bang & olufsen", DeviceType::Speaker),
    ("sonos", DeviceType::Speaker),
    ("skullcandy", DeviceType::Audio),
    ("audio-technica", DeviceType::Audio),
    ("plantronics", DeviceType::Audio),
    ("anker", DeviceType::Audio),
    // Watches / Wearables
    ("fitbit", DeviceType::Watch),
    ("garmin", DeviceType::Watch),
    ("polar", DeviceType::Watch),
    ("suunto", DeviceType::Watch),
    ("whoop", DeviceType::Wearable),
    ("oura", DeviceType::Wearable),
    // Smart Home
    ("amazon", DeviceType::SmartHome),
    ("ring", DeviceType::SmartHome),
    ("nest", DeviceType::SmartHome),
    ("philips", DeviceType::SmartHome),
    ("ikea", DeviceType::SmartHome),
    ("tuya", DeviceType::SmartHome),
    ("shelly", DeviceType::SmartHome),
    ("switchbot", DeviceType::SmartHome),
    ("aqara", DeviceType::SmartHome),
    ("wyze", DeviceType::SmartHome),
    ("eufy", DeviceType::SmartHome),
    ("ecobee", DeviceType::SmartHome),
    ("hue", DeviceType::SmartHome),
    ("smartthings", DeviceType::SmartHome),
    ("tp-link", DeviceType::SmartHome),
    ("meross", DeviceType::SmartHome),
    ("govee", DeviceType::SmartHome),
    ("lifx", DeviceType::SmartHome),
    ("nanoleaf", DeviceType::SmartHome),
    ("yale", DeviceType::SmartHome),
    ("august", DeviceType::SmartHome),
    ("schlage", DeviceType::SmartHome),
    ("espressif", DeviceType::SmartHome),
    // TV
    ("roku", DeviceType::Tv),
    ("vizio", DeviceType::Tv),
    ("tcl", DeviceType::Tv),
    ("hisense", DeviceType::Tv),
    ("chromecast", DeviceType::Tv),
    ("fire tv", DeviceType::Tv),
    // Vehicles
    ("tesla", DeviceType::Vehicle),
    ("ford", DeviceType::Vehicle),
    ("gm", DeviceType::Vehicle),
    ("volkswagen", DeviceType::Vehicle),
    ("bmw", DeviceType::Vehicle),
    ("mercedes", DeviceType::Vehicle),
    ("audi", DeviceType::Vehicle),
    ("toyota", DeviceType::Vehicle),
    ("honda", DeviceType::Vehicle),
    ("nissan", DeviceType::Vehicle),
    ("hyundai", DeviceType::Vehicle),
    ("kia", DeviceType::Vehicle),
    ("volvo", DeviceType::Vehicle),
    ("rivian", DeviceType::Vehicle),
    ("lucid", DeviceType::Vehicle),
    ("harley", DeviceType::Vehicle),
    ("continental auto", DeviceType::Vehicle),
    ("bosch", DeviceType::Vehicle),
    ("denso", DeviceType::Vehicle),
    // Gaming
    ("nintendo", DeviceType::Gaming),
    ("playstation", DeviceType::Gaming),
    ("xbox", DeviceType::Gaming),
    ("valve", DeviceType::Gaming),
    ("razer", DeviceType::Gaming),
    ("steelseries", DeviceType::Gaming),
    ("logitech", DeviceType::Gaming),
    // Camera
    ("gopro", DeviceType::Camera),
    ("canon", DeviceType::Camera),
    ("nikon", DeviceType::Camera),
    ("dji", DeviceType::Camera),
    ("insta360", DeviceType::Camera),
    // Printers
    ("epson", DeviceType::Printer),
    ("brother", DeviceType::Printer),
    ("xerox", DeviceType::Printer),
    // Network
    ("cisco", DeviceType::Network),
    ("netgear", DeviceType::Network),
    ("ubiquiti", DeviceType::Network),
    ("aruba", DeviceType::Network),
    ("linksys", DeviceType::Network),
    ("asus router", DeviceType::Network),
    ("eero", DeviceType::Network),
    ("orbi", DeviceType::Network),
];

/// Classify a device using all available data, in priority order:
/// Service UUIDs > Manufacturer data > Name patterns > Device class > Vendor patterns
pub fn classify_device(
    vendor: Option<&str>,
    name: Option<&str>,
    service_uuids: &[String],
    manufacturer_data: &HashMap<u16, Vec<u8>>,
    device_class: Option<u32>,
) -> DeviceType {
    // 1. Service UUIDs (most specific)
    for uuid in service_uuids {
        let normalized = uuid.to_lowercase().replace('-', "");
        for &(pattern, dtype) in SERVICE_UUID_PATTERNS {
            if normalized.contains(pattern) {
                return dtype;
            }
        }
    }

    // 2. Manufacturer data (company ID + Apple sub-types)
    if let Some(dt) = classify_by_manufacturer_data(manufacturer_data) {
        return dt;
    }

    // 3. Name patterns
    if let Some(name) = name {
        let name_lower = name.to_lowercase();
        for &(patterns, dtype) in NAME_PATTERNS {
            if patterns.iter().any(|p| name_lower.contains(p)) {
                return dtype;
            }
        }
    }

    // 4. Classic Bluetooth device class
    if let Some(class) = device_class {
        if let Some(dt) = classify_by_device_class(class) {
            return dt;
        }
    }

    // 5. Vendor patterns (OUI or manufacturer data company name)
    if let Some(vendor) = vendor {
        let vendor_lower = vendor.to_lowercase();
        for &(pattern, dtype) in VENDOR_PATTERNS {
            if vendor_lower.contains(pattern) {
                return dtype;
            }
        }
    }

    DeviceType::Unknown
}

/// Check if a MAC address uses a locally-administered (randomized) address.
pub fn is_randomized_mac(mac: &str) -> bool {
    let Some(first) = mac.split(':').next() else {
        return false;
    };
    let Ok(byte) = u8::from_str_radix(first, 16) else {
        return false;
    };
    byte & 0x02 != 0
}

/// BLE address type parsed from the MAC's first octet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleAddrType {
    /// IEEE-assigned OUI, globally unique (bit 1 = 0)
    Public,
    /// Random static — persists across power cycles (top 2 bits = 11)
    RandomStatic,
    /// Resolvable private — rotates, resolvable via IRK (top 2 bits = 01)
    ResolvablePrivate,
    /// Non-resolvable private — fully random (top 2 bits = 00)
    NonResolvablePrivate,
    /// Multicast bit set — anomalous for BLE (bit 0 = 1)
    Multicast,
}

impl BleAddrType {
    pub fn to_db(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::RandomStatic => "random_static",
            Self::ResolvablePrivate => "resolvable_private",
            Self::NonResolvablePrivate => "non_resolvable_private",
            Self::Multicast => "multicast",
        }
    }

    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "random_static" => Some(Self::RandomStatic),
            "resolvable_private" => Some(Self::ResolvablePrivate),
            "non_resolvable_private" => Some(Self::NonResolvablePrivate),
            "multicast" => Some(Self::Multicast),
            _ => None,
        }
    }
}

/// Parse BLE address type from the first octet of a MAC address.
///
/// First octet bits:
/// - Bit 0: unicast (0) / multicast (1) — multicast is anomalous in BLE
/// - Bit 1: universally administered (0) / locally administered (1)
/// For locally-administered (random) addresses, the top 2 bits of the
/// first octet identify the random subtype:
/// - 11: Random Static
/// - 01: Resolvable Private Address (RPA)
/// - 00: Non-Resolvable Private
pub fn parse_addr_type(mac: &str) -> Option<BleAddrType> {
    let first = mac.split(':').next()?;
    let byte = u8::from_str_radix(first, 16).ok()?;
    if byte & 0x01 != 0 {
        return Some(BleAddrType::Multicast);
    }
    if byte & 0x02 == 0 {
        return Some(BleAddrType::Public);
    }
    // Locally administered → check random subtype from top 2 bits
    match byte >> 6 {
        0b11 => Some(BleAddrType::RandomStatic),
        0b01 => Some(BleAddrType::ResolvablePrivate),
        _ => Some(BleAddrType::NonResolvablePrivate),
    }
}

/// Compute a short fingerprint for a device based on its advertising signature.
///
/// Devices that rotate their MAC address will still produce the same fingerprint
/// if they advertise the same company IDs, service UUIDs, name, and tx_power.
/// This lets us correlate multiple randomized MACs to the same physical device.
///
/// Returns a 4-char hex string like "A3F2".
pub fn compute_fingerprint(
    name: Option<&str>,
    service_uuids: &[String],
    manufacturer_data: &HashMap<u16, Vec<u8>>,
    tx_power: Option<i16>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    // Manufacturer company IDs (sorted for stability)
    let mut company_ids: Vec<u16> = manufacturer_data.keys().copied().collect();
    company_ids.sort();
    for id in &company_ids {
        id.hash(&mut hasher);
    }

    // For Apple: include continuity type byte (more stable than full payload)
    if let Some(data) = manufacturer_data.get(&0x004C) {
        if let Some(&type_byte) = data.first() {
            type_byte.hash(&mut hasher);
        }
    }

    // Service UUIDs (sorted, first 8 chars for short UUIDs)
    let mut uuids: Vec<String> = service_uuids
        .iter()
        .map(|u| {
            let normalized = u.to_lowercase().replace('-', "");
            normalized.chars().take(8).collect()
        })
        .collect();
    uuids.sort();
    for uuid in &uuids {
        uuid.hash(&mut hasher);
    }

    // Device name (often stable across rotations)
    if let Some(n) = name {
        n.hash(&mut hasher);
    }

    // TX power (hardware-specific, stable per device model)
    if let Some(tp) = tx_power {
        tp.hash(&mut hasher);
    }

    let hash = hasher.finish();
    format!("{:04X}", (hash & 0xFFFF) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── company_name ─────────────────────────────────────────────────────

    #[test]
    fn company_name_apple() {
        assert_eq!(company_name(0x004C), Some("Apple"));
    }

    #[test]
    fn company_name_samsung() {
        assert_eq!(company_name(0x0075), Some("Samsung"));
    }

    #[test]
    fn company_name_unknown() {
        assert_eq!(company_name(0xFFFF), None);
    }

    // ── best_company_name ────────────────────────────────────────────────

    #[test]
    fn best_company_name_recognized() {
        let mut mfr = HashMap::new();
        mfr.insert(0x004C, vec![0x07, 0x00]);
        assert_eq!(best_company_name(&mfr), Some("Apple".to_string()));
    }

    #[test]
    fn best_company_name_unrecognized_hex() {
        let mut mfr = HashMap::new();
        mfr.insert(0xBEEF, vec![]);
        assert_eq!(best_company_name(&mfr), Some("BT#BEEF".to_string()));
    }

    #[test]
    fn best_company_name_empty() {
        let mfr = HashMap::new();
        assert_eq!(best_company_name(&mfr), None);
    }

    // ── is_randomized_mac ────────────────────────────────────────────────

    #[test]
    fn randomized_mac_true() {
        // 0x5E → bit 1 set → locally administered
        assert!(is_randomized_mac("5E:AA:BB:CC:DD:EE"));
    }

    #[test]
    fn randomized_mac_false() {
        // 0xAC → bit 1 not set → public (global) address
        assert!(!is_randomized_mac("AC:DE:48:00:11:22"));
    }

    #[test]
    fn randomized_mac_invalid() {
        assert!(!is_randomized_mac(""));
        assert!(!is_randomized_mac("ZZ:11:22:33:44:55"));
    }

    // ── parse_addr_type ───────────────────────────────────────────────

    #[test]
    fn addr_type_public() {
        // 0xAC = 10101100 → bit1=0 (universal), bit0=0 (unicast)
        assert_eq!(parse_addr_type("AC:DE:48:00:11:22"), Some(BleAddrType::Public));
    }

    #[test]
    fn addr_type_random_static() {
        // 0xDE = 11011110 → bit1=1 (local), top 2 bits = 11
        assert_eq!(parse_addr_type("DE:AA:BB:CC:DD:EE"), Some(BleAddrType::RandomStatic));
    }

    #[test]
    fn addr_type_resolvable_private() {
        // 0x5E = 01011110 → bit1=1 (local), top 2 bits = 01
        assert_eq!(parse_addr_type("5E:AA:BB:CC:DD:EE"), Some(BleAddrType::ResolvablePrivate));
    }

    #[test]
    fn addr_type_non_resolvable() {
        // 0x1E = 00011110 → bit1=1 (local), top 2 bits = 00
        assert_eq!(parse_addr_type("1E:AA:BB:CC:DD:EE"), Some(BleAddrType::NonResolvablePrivate));
    }

    #[test]
    fn addr_type_multicast() {
        // 0xAD = 10101101 → bit0=1 (multicast)
        assert_eq!(parse_addr_type("AD:DE:48:00:11:22"), Some(BleAddrType::Multicast));
    }

    #[test]
    fn addr_type_invalid() {
        assert_eq!(parse_addr_type(""), None);
        assert_eq!(parse_addr_type("ZZ:11:22:33:44:55"), None);
    }

    // ── BleAddrType to_db/from_db round-trip ─────────────────────────────

    #[test]
    fn addr_type_round_trip_all_variants() {
        let variants = [
            BleAddrType::Public,
            BleAddrType::RandomStatic,
            BleAddrType::ResolvablePrivate,
            BleAddrType::NonResolvablePrivate,
            BleAddrType::Multicast,
        ];
        for variant in variants {
            let db_str = variant.to_db();
            let parsed = BleAddrType::from_db(db_str);
            assert_eq!(parsed, Some(variant), "Round-trip failed for {db_str}");
        }
    }

    #[test]
    fn addr_type_from_db_invalid() {
        assert_eq!(BleAddrType::from_db("garbage"), None);
        assert_eq!(BleAddrType::from_db(""), None);
    }

    // ── compute_fingerprint ──────────────────────────────────────────────

    #[test]
    fn fingerprint_deterministic() {
        let mut mfr = HashMap::new();
        mfr.insert(0x004C, vec![0x07, 0x19]);
        let uuids = vec!["0000180a-0000-1000-8000-00805f9b34fb".to_string()];
        let fp1 = compute_fingerprint(Some("MyDevice"), &uuids, &mfr, Some(4));
        let fp2 = compute_fingerprint(Some("MyDevice"), &uuids, &mfr, Some(4));
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_differs_for_different_inputs() {
        let mfr = HashMap::new();
        let fp1 = compute_fingerprint(Some("DeviceA"), &[], &mfr, None);
        let fp2 = compute_fingerprint(Some("DeviceB"), &[], &mfr, None);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn fingerprint_is_4_char_hex() {
        let mfr = HashMap::new();
        let fp = compute_fingerprint(None, &[], &mfr, None);
        assert_eq!(fp.len(), 4);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── classify_apple_mfr_data ──────────────────────────────────────────

    #[test]
    fn apple_ibeacon() {
        assert_eq!(classify_apple_mfr_data(&[0x02]), DeviceType::SmartHome);
    }

    #[test]
    fn apple_airpods() {
        assert_eq!(classify_apple_mfr_data(&[0x07]), DeviceType::Audio);
    }

    #[test]
    fn apple_airplay() {
        assert_eq!(classify_apple_mfr_data(&[0x09]), DeviceType::Speaker);
    }

    #[test]
    fn apple_handoff() {
        assert_eq!(classify_apple_mfr_data(&[0x0C]), DeviceType::Computer);
    }

    #[test]
    fn apple_findmy() {
        assert_eq!(classify_apple_mfr_data(&[0x19]), DeviceType::SmartHome);
    }

    #[test]
    fn apple_default_phone() {
        assert_eq!(classify_apple_mfr_data(&[0x10]), DeviceType::Phone);
    }

    #[test]
    fn apple_empty_data() {
        assert_eq!(classify_apple_mfr_data(&[]), DeviceType::Phone);
    }

    // ── classify_by_device_class ─────────────────────────────────────────

    #[test]
    fn device_class_computer() {
        // Major class 1 = Computer (bits 12-8 = 0b00001)
        assert_eq!(classify_by_device_class(0x100), Some(DeviceType::Computer));
    }

    #[test]
    fn device_class_phone() {
        // Major class 2 = Phone
        assert_eq!(classify_by_device_class(0x200), Some(DeviceType::Phone));
    }

    #[test]
    fn device_class_audio() {
        // Major class 4 = Audio/Video
        assert_eq!(classify_by_device_class(0x400), Some(DeviceType::Audio));
    }

    #[test]
    fn device_class_unknown_major() {
        // Major class 0 = Miscellaneous
        assert_eq!(classify_by_device_class(0x000), None);
    }

    // ── classify_device priority ─────────────────────────────────────────

    #[test]
    fn classify_service_uuid_highest_priority() {
        let mut mfr = HashMap::new();
        mfr.insert(0x004C, vec![0x07]); // would classify as Audio via Apple
        let uuids = vec!["0000180d-0000-1000-8000-00805f9b34fb".to_string()]; // Heart Rate → Wearable
        let dt = classify_device(None, None, &uuids, &mfr, None);
        assert_eq!(dt, DeviceType::Wearable); // UUID wins over manufacturer data
    }

    #[test]
    fn classify_manufacturer_over_name() {
        let mut mfr = HashMap::new();
        mfr.insert(0x009E, vec![]); // Bose → Audio
        let dt = classify_device(None, Some("TV Living Room"), &[], &mfr, None);
        assert_eq!(dt, DeviceType::Audio); // Mfr data wins over name pattern "TV"
    }

    #[test]
    fn classify_name_pattern() {
        let mfr = HashMap::new();
        let dt = classify_device(None, Some("Matt's AirPods"), &[], &mfr, None);
        assert_eq!(dt, DeviceType::Audio);
    }

    #[test]
    fn classify_vendor_fallback() {
        let mfr = HashMap::new();
        let dt = classify_device(Some("Tesla, Inc."), None, &[], &mfr, None);
        assert_eq!(dt, DeviceType::Vehicle);
    }

    #[test]
    fn classify_unknown_fallback() {
        let mfr = HashMap::new();
        let dt = classify_device(None, None, &[], &mfr, None);
        assert_eq!(dt, DeviceType::Unknown);
    }
}

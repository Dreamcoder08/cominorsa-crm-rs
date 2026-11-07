//! Prepared chat links. A person always reviews and sends the message.

const PERU_CODE: &str = "51";
const PERU_MOBILE_DIGITS: usize = 9;
const INTERNATIONAL_DIGITS: std::ops::RangeInclusive<usize> = 8..=15;

/// Returns the international number for a phone that can use WhatsApp.
///
/// Numbers without a `+` are read as Peruvian, where mobiles have nine digits
/// and start with 9.
pub fn whatsapp_number(phone: &str) -> Option<String> {
    let phone = phone.trim();
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();

    if phone.starts_with('+') {
        return INTERNATIONAL_DIGITS
            .contains(&digits.len())
            .then_some(digits);
    }

    let is_mobile = |value: &str| value.len() == PERU_MOBILE_DIGITS && value.starts_with('9');
    if is_mobile(&digits) {
        return Some(format!("{PERU_CODE}{digits}"));
    }
    digits
        .strip_prefix(PERU_CODE)
        .filter(|local| is_mobile(local))
        .map(|_| digits.clone())
}

pub fn render_message(template: &str, name: &str, service: &str) -> String {
    template
        .replace("{nombre}", name)
        .replace("{servicio}", service)
}

pub(crate) fn has_only_known_placeholders(template: &str) -> bool {
    let without_known = render_message(template, "", "");
    !without_known.contains('{') && !without_known.contains('}')
}

pub(crate) fn chat_url(number: &str, message: &str) -> String {
    format!("https://wa.me/{number}?text={}", encode(message))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

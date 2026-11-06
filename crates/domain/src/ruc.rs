use crate::DomainError;

const LENGTH: usize = 11;
const VALID_PREFIXES: [&str; 5] = ["10", "15", "16", "17", "20"];
const WEIGHTS: [u32; 10] = [5, 4, 3, 2, 7, 6, 5, 4, 3, 2];

/// Peruvian taxpayer number with a valid format and check digit.
///
/// Validation is offline, so it does not prove the number exists at SUNAT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruc(String);

impl Ruc {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let digits: Vec<u32> = value.chars().filter_map(|c| c.to_digit(10)).collect();
        let has_valid_shape = value.len() == LENGTH && digits.len() == LENGTH;

        if has_valid_shape
            && VALID_PREFIXES.iter().any(|prefix| value.starts_with(prefix))
            && check_digit(&digits) == digits[LENGTH - 1]
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(DomainError::InvalidRuc)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn check_digit(digits: &[u32]) -> u32 {
    let sum: u32 = digits.iter().zip(WEIGHTS).map(|(d, w)| d * w).sum();

    match 11 - sum % 11 {
        10 => 0,
        11 => 1,
        digit => digit,
    }
}

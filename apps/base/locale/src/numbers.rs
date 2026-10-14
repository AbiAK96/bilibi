//! Regional number formatting from CLDR data (ICU4X compiled data), keyed by a
//! catalogue locale. Values are decimal strings or integers, never floats, so
//! no precision is lost before the locale chooses separators, digits and
//! grouping (`hi-IN` groups in lakhs, `de-CH` uses an apostrophe, `ar-SA` uses
//! its own digits). This is formatting only: copy is not translated.
use crate::catalog;
use bilibi_database::{Error, Result};
use icu_decimal::{input::Decimal, DecimalFormatter};
use icu_locale::Locale;

pub struct Numbers {
    code: &'static str,
    formatter: DecimalFormatter,
}

impl Numbers {
    /// `locale` must already be a supported catalogue code (see `normalize`).
    pub fn new(locale: &str) -> Result<Self> {
        let entry = catalog::find(locale).ok_or(Error::Invalid)?;
        let parsed: Locale = entry.code.parse().map_err(|_| Error::Internal)?;
        let formatter = DecimalFormatter::try_new((&parsed).into(), Default::default())
            .map_err(|_| Error::Internal)?;
        Ok(Self {
            code: entry.code,
            formatter,
        })
    }

    pub fn locale(&self) -> &'static str {
        self.code
    }

    pub fn integer(&self, value: i64) -> String {
        self.formatter.format(&Decimal::from(value)).to_string()
    }

    /// `value` is a plain decimal literal such as `-1234567.891`. With
    /// `fraction_digits`, the value is rounded half-even then zero padded to
    /// exactly that many digits; without, its written precision is kept.
    pub fn decimal(&self, value: &str, fraction_digits: Option<u8>) -> Result<String> {
        let mut decimal = Decimal::try_from_str(value).map_err(|_| Error::Invalid)?;
        if let Some(digits) = fraction_digits {
            if digits > 20 {
                return Err(Error::Invalid);
            }
            let position = -i16::from(digits);
            decimal.round(position);
            decimal.pad_end(position);
        }
        Ok(self.formatter.format(&decimal).to_string())
    }
}

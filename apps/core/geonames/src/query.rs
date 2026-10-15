//! Bounded index queries. Caller text never becomes SQL: sort keys resolve
//! through the closed enums below to static fragments, everything else is a
//! bind parameter, and every list is capped.
use std::collections::HashMap;

/// Allowed page sizes; a request rounds up to the next one, past the last it
/// is the last.
pub const PAGE_SIZES: [u64; 4] = [25, 50, 100, 300];
pub const MAX_PAGE: u64 = 1_000_000;
/// Longest search text any index or lookup honours.
pub const MAX_SEARCH: usize = 255;
/// Ceiling for the reference lists that are small by nature (countries, one
/// country's divisions, per-country postcode totals).
pub const MAX_LIST: i64 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dir {
    Asc,
    Desc,
}
impl Dir {
    pub fn key(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
        }
    }
    pub(crate) fn sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

macro_rules! sorts {
    ($name:ident, default $default:ident, desc [$($desc:ident),*], { $($variant:ident => $key:literal, $sql:literal;)+ }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            /// The URL/event key of the sortable column.
            pub fn key(self) -> &'static str { match self { $(Self::$variant => $key),+ } }
            pub fn parse(value: &str) -> Option<Self> { Self::ALL.iter().copied().find(|sort| sort.key() == value) }
            /// The direction a first click on this column sorts in.
            pub fn initial(self) -> Dir {
                match self { $(Self::$desc)|* => Dir::Desc, _ => Dir::Asc }
            }
            pub(crate) fn sql(self) -> &'static str { match self { $(Self::$variant => $sql),+ } }
            /// The requested sort, or the default for an unknown or absent key.
            pub fn resolve(requested: Option<&str>) -> Self {
                requested.and_then(Self::parse).unwrap_or(Self::$default)
            }
        }
    };
}
sorts!(CountrySort, default Country, desc [Population, UpdatedAt], {
    Iso => "iso", "c.iso";
    Country => "country", "c.country";
    Capital => "capital", "c.capital";
    Phone => "phone", "c.phone";
    CurrencyCode => "currency_code", "c.currency_code";
    Population => "population", "c.population";
    UpdatedAt => "updated_at", "c.updated_at";
});
sorts!(Admin1Sort, default CountryName, desc [UpdatedAt], {
    CountryName => "country_name", "c.country";
    Code => "code", "a.code";
    Name => "name", "a.name";
    AltName => "alt_name", "a.alt_name";
    UpdatedAt => "updated_at", "a.updated_at";
});
sorts!(PostcodeSort, default CountryName, desc [UpdatedAt], {
    CountryName => "country_name", "c.country";
    Postcode => "postcode", "p.postcode";
    PlaceName => "place_name", "p.place_name";
    Admin1Code => "admin1_code", "p.admin1_code";
    UpdatedAt => "updated_at", "p.updated_at";
});
sorts!(SummarySort, default CountryName, desc [RecordCount], {
    CountryName => "country_name", "COALESCE(c.country, p.country_iso)";
    CountryIso => "country_iso", "p.country_iso";
    RecordCount => "record_count", "count(*)";
});

/// What an index asks for, before normalisation. Every field is untrusted:
/// the index functions clamp, default or empty it, never fail on it.
#[derive(Clone, Debug, Default)]
pub struct IndexQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_dir: Option<String>,
    /// Admin1 only. Blank is no filter; a malformed ISO matches nothing.
    pub country_iso: Option<String>,
}
impl IndexQuery {
    /// The query string / form keys a page passes through, with their legacy
    /// aliases.
    pub fn from_params(params: &HashMap<String, String>) -> Self {
        let get = |names: &[&str]| names.iter().find_map(|name| params.get(*name));
        let number = |names: &[&str]| get(names).and_then(|value| value.trim().parse::<u64>().ok());
        Self {
            page: number(&["page"]),
            page_size: number(&["page_size", "per_page", "perPage"]),
            search: get(&["search"]).cloned(),
            sort_by: get(&["sort_by", "sortBy", "sort"]).cloned(),
            sort_dir: get(&["sort_dir", "sortDir", "direction"]).cloned(),
            country_iso: get(&["country_iso", "filter_country_iso", "filterCountryIso"]).cloned(),
        }
    }
}

/// An `IndexQuery` after normalisation.
pub(crate) struct Normal<S> {
    pub page: u64,
    pub page_size: u64,
    pub search: Option<String>,
    pub sort: S,
    pub dir: Dir,
}
macro_rules! normalise {
    ($sort:ident) => {
        impl Normal<$sort> {
            pub(crate) fn of(query: &IndexQuery) -> Self {
                let sort = $sort::resolve(query.sort_by.as_deref());
                Normal {
                    page: query
                        .page
                        .filter(|page| *page > 0)
                        .unwrap_or(1)
                        .min(MAX_PAGE),
                    page_size: page_size(query.page_size),
                    search: search(query.search.as_deref()),
                    sort,
                    dir: query
                        .sort_dir
                        .as_deref()
                        .and_then(Dir::parse)
                        .unwrap_or_else(|| sort.initial()),
                }
            }
        }
    };
}
normalise!(CountrySort);
normalise!(Admin1Sort);
normalise!(PostcodeSort);
normalise!(SummarySort);

pub(crate) fn page_size(requested: Option<u64>) -> u64 {
    let wanted = requested.filter(|size| *size > 0).unwrap_or(PAGE_SIZES[0]);
    PAGE_SIZES
        .iter()
        .copied()
        .find(|size| *size >= wanted)
        .unwrap_or(PAGE_SIZES[PAGE_SIZES.len() - 1])
}

/// Trimmed, bounded search text; blank is no search.
pub(crate) fn search(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    (!text.is_empty()).then(|| text.chars().take(MAX_SEARCH).collect())
}

/// `%text%` with LIKE metacharacters made literal.
pub(crate) fn contains_pattern(text: &str) -> String {
    format!("%{}%", escape_like(text))
}
pub(crate) fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// A stable bounded page. `total_pages` is 0 for an empty result.
#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    pub entries: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total_entries: u64,
    pub total_pages: u64,
}
impl<T> Page<T> {
    pub(crate) fn new(entries: Vec<T>, page: u64, page_size: u64, total_entries: u64) -> Self {
        Self {
            entries,
            page,
            page_size,
            total_entries,
            total_pages: total_entries.div_ceil(page_size),
        }
    }
}

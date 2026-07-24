use jiff::civil::Date;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteCategory {
    Permanent,
    Time,
    Capture,
    Generated,
}

impl NoteCategory {
    pub fn from_dir(dir: &str) -> Option<NoteCategory> {
        match dir {
            "permanent" => Some(NoteCategory::Permanent),
            "time" => Some(NoteCategory::Time),
            "capture" => Some(NoteCategory::Capture),
            "generated" => Some(NoteCategory::Generated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteType {
    Person,
    Organisation,
    Source,
    Concept,
    Claim,
    Idea,
    Personal,
    Project,
    Daily,
    Weekly,
    Seasonal,
    Generated,
    Unknown(String),
}

impl NoteType {
    pub fn from_name(name: &str) -> NoteType {
        match name {
            "person" => NoteType::Person,
            "organisation" => NoteType::Organisation,
            "source" => NoteType::Source,
            "concept" => NoteType::Concept,
            "claim" => NoteType::Claim,
            "idea" => NoteType::Idea,
            "personal" => NoteType::Personal,
            "project" => NoteType::Project,
            "daily" => NoteType::Daily,
            "weekly" => NoteType::Weekly,
            "seasonal" => NoteType::Seasonal,
            "generated" => NoteType::Generated,
            unknown_name => NoteType::Unknown(unknown_name.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaStatus {
    Missing,
    Present(Meta),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    pub id: Option<NoteId>,
    pub note_type: Option<NoteType>,
    pub created: Option<Date>,
    pub tags: Vec<String>,
    pub origin: Option<String>,
    pub anomalies: Vec<MetaAnomaly>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaAnomaly {
    DuplicateMeta,
    InvalidCreated(String),         // raw text
    MalformedField(String, String), // (field name, raw text)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub target: NoteId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub path: std::path::PathBuf, // vault-relative
    pub category: NoteCategory,
    pub meta: MetaStatus,
    pub links: Vec<Link>,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn categories_map_from_their_directory_names() {
        assert_eq!(
            NoteCategory::from_dir("permanent"),
            Some(NoteCategory::Permanent)
        );
        assert_eq!(NoteCategory::from_dir("time"), Some(NoteCategory::Time));
        assert_eq!(
            NoteCategory::from_dir("capture"),
            Some(NoteCategory::Capture)
        );
        assert_eq!(
            NoteCategory::from_dir("generated"),
            Some(NoteCategory::Generated)
        );
    }

    #[test]
    fn non_category_directories_are_not_categories() {
        assert_eq!(NoteCategory::from_dir("templates"), None);
        assert_eq!(NoteCategory::from_dir(".index"), None);
        assert_eq!(NoteCategory::from_dir(""), None);
    }

    #[test]
    fn every_known_type_name_maps_to_its_variant() {
        let cases = [
            ("person", NoteType::Person),
            ("organisation", NoteType::Organisation),
            ("source", NoteType::Source),
            ("concept", NoteType::Concept),
            ("claim", NoteType::Claim),
            ("idea", NoteType::Idea),
            ("personal", NoteType::Personal),
            ("project", NoteType::Project),
            ("daily", NoteType::Daily),
            ("weekly", NoteType::Weekly),
            ("seasonal", NoteType::Seasonal),
            ("generated", NoteType::Generated),
        ];
        for (name, expected) in cases {
            assert_eq!(NoteType::from_name(name), expected, "for name {name:?}");
        }
    }

    #[test]
    fn unknown_type_names_are_kept_verbatim() {
        assert_eq!(
            NoteType::from_name("concpet"),
            NoteType::Unknown("concpet".to_string())
        );
        assert_eq!(NoteType::from_name(""), NoteType::Unknown(String::new()));
        // case matters: types are lowercase by convention, a wrong case is a typo
        assert_eq!(
            NoteType::from_name("Concept"),
            NoteType::Unknown("Concept".to_string())
        );
    }
}

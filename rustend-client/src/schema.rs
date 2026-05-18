#[derive(Debug, Clone, Default)]
pub struct IndexEntry {
    pub name:        String,
    pub object_type: String,
    pub json_path:   String,
}

#[derive(Debug, Clone, Default)]
pub struct IndexSchema {
    pub entries: Vec<IndexEntry>,
}

impl IndexSchema {
    pub fn new() -> Self { Self::default() }

    pub fn add(
        mut self,
        name: impl Into<String>,
        object_type: impl Into<String>,
        json_path: impl Into<String>,
    ) -> Self {
        self.entries.push(IndexEntry {
            name:        name.into(),
            object_type: object_type.into(),
            json_path:   json_path.into(),
        });
        self
    }
}

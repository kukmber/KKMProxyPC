use std::path::PathBuf;

#[derive(Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub cores: PathBuf,
    pub profiles: PathBuf,
    /// Рабочая папка mihomo (`-d`): конфиг, правила, geo-базы, cache.db.
    pub mihomo_home: PathBuf,
    /// Папка ресурсов установщика: туда кладутся ядра при сборке релиза.
    pub resources: Option<PathBuf>,
}

impl Paths {
    pub fn new(root: PathBuf, resources: Option<PathBuf>) -> std::io::Result<Self> {
        let p = Self {
            cores: root.join("cores"),
            profiles: root.join("profiles"),
            mihomo_home: root.join("mihomo"),
            root,
            resources,
        };
        for dir in [&p.root, &p.cores, &p.profiles, &p.mihomo_home] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(p)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.root.join("profiles.json")
    }

    pub fn profile_content(&self, id: &str) -> PathBuf {
        self.profiles.join(format!("{id}.yaml"))
    }
}

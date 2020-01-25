pub mod cli;

use std::{
    collections::HashMap,
    convert::From,
    env, fmt,
    num::ParseIntError,
    path::{Path, PathBuf},
    str::FromStr,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq)]
pub enum Error {
    // {RequestedVersion, ExactVersion}::from_str
    ParseVersionComponentError(ParseIntError),
    // RequestedVersion::from_str
    DotMissing,
    // ExactVersion::from_path
    FileNameMissing,
    FileNameToStrError,
    PathFileNameError,
    // cli::{list_executables, find_executable, help}
    NoExecutableFound(RequestedVersion),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ParseVersionComponentError(int_error) => {
                write!(f, "Error parsing a version component: {}", int_error)
            }
            Self::DotMissing => write!(f, "'.' missing from the version"),
            Self::FileNameMissing => write!(f, "Path lacks a file name"),
            Self::FileNameToStrError => write!(f, "Failed to convert file name to `str`"),
            Self::PathFileNameError => write!(f, "File name not of the format `pythonX.Y`"),
            Self::NoExecutableFound(requested_version) => write!(
                f,
                "No executable found for {}",
                requested_version.to_string()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ParseVersionComponentError(int_error) => Some(int_error),
            Self::DotMissing => None,
            Self::FileNameMissing => None,
            Self::FileNameToStrError => None,
            Self::PathFileNameError => None,
            Self::NoExecutableFound(_) => None,
        }
    }
}

/// An integral part of a version specifier (e.g. the `X` or `Y` of `X.Y`).
type ComponentSize = u16;

/// Represents the version of Python a user requsted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RequestedVersion {
    Any,
    MajorOnly(ComponentSize),
    Exact(ComponentSize, ComponentSize),
}

impl ToString for RequestedVersion {
    fn to_string(&self) -> String {
        match self {
            Self::Any => "Python".to_string(),
            Self::MajorOnly(major) => format!("Python {}", major),
            Self::Exact(major, minor) => format!("Python {}.{}", major, minor),
        }
    }
}

impl FromStr for RequestedVersion {
    type Err = Error;

    // XXX Require `python` as a prefix?
    fn from_str(version_string: &str) -> Result<Self> {
        if version_string.is_empty() {
            Ok(Self::Any)
        } else if version_string.contains('.') {
            let exact_version = ExactVersion::from_str(version_string)?;
            Ok(Self::Exact(exact_version.major, exact_version.minor))
        } else {
            match version_string.parse::<ComponentSize>() {
                Ok(number) => Ok(Self::MajorOnly(number)),
                Err(parse_error) => Err(Error::ParseVersionComponentError(parse_error)),
            }
        }
    }
}

impl RequestedVersion {
    /// Returns the string representing the environment variable for the requested version.
    pub fn env_var(self) -> Option<String> {
        match self {
            Self::Any => Some("PY_PYTHON".to_string()),
            Self::MajorOnly(major) => Some(format!("PY_PYTHON{}", major)),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExactVersion {
    pub major: ComponentSize,
    pub minor: ComponentSize,
}

impl From<ExactVersion> for RequestedVersion {
    fn from(version: ExactVersion) -> Self {
        Self::Exact(version.major, version.minor)
    }
}

impl ToString for ExactVersion {
    fn to_string(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl FromStr for ExactVersion {
    type Err = Error;

    fn from_str(version_string: &str) -> Result<Self> {
        if let Some(dot_index) = version_string.find('.') {
            let major_str = &version_string[..dot_index];
            let major = match major_str.parse::<ComponentSize>() {
                Ok(number) => number,
                Err(parse_error) => return Err(Error::ParseVersionComponentError(parse_error)),
            };

            let minor_str = &version_string[dot_index + 1..];
            match minor_str.parse::<ComponentSize>() {
                Ok(minor) => Ok(Self { major, minor }),
                Err(parse_error) => Err(Error::ParseVersionComponentError(parse_error)),
            }
        } else {
            Err(Error::DotMissing)
        }
    }
}

impl ExactVersion {
    pub fn from_path(path: &Path) -> Result<Self> {
        if let Some(raw_file_name) = path.file_name() {
            if let Some(file_name) = raw_file_name.to_str() {
                if file_name.len() >= "python3.0".len() && file_name.starts_with("python") {
                    let version_part = &file_name["python".len()..];
                    return Self::from_str(version_part);
                }
                Err(Error::PathFileNameError)
            } else {
                Err(Error::FileNameToStrError)
            }
        } else {
            Err(Error::FileNameMissing)
        }
    }

    // XXX from_shebang()?

    pub fn supports(&self, requested: RequestedVersion) -> bool {
        match requested {
            RequestedVersion::Any => true,
            RequestedVersion::MajorOnly(major_version) => self.major == major_version,
            RequestedVersion::Exact(major_version, minor_version) => {
                self.major == major_version && self.minor == minor_version
            }
        }
    }
}

fn env_path() -> Vec<PathBuf> {
    // Would love to have a return type of `impl Iterator<Item = PathBuf>
    // and return just SplitPaths and iter::empty(), but Rust
    // complains about differing return types.
    match env::var_os("PATH") {
        Some(path_val) => env::split_paths(&path_val).collect(),
        None => Vec::new(),
    }
}

fn flatten_directories(
    directories: impl IntoIterator<Item = PathBuf>,
) -> impl Iterator<Item = PathBuf> {
    directories
        .into_iter()
        .filter_map(|p| p.read_dir().ok()) // Filter to Ok(ReadDir).
        .flatten() // Flatten out `for DirEntry in ReadDir`.
        .filter_map(|e| e.ok()) // Filter to Ok(DirEntry).
        .map(|e| e.path()) // Get the PathBuf from the DirEntry.
}

fn all_executables_in_paths(
    paths: impl IntoIterator<Item = PathBuf>,
) -> HashMap<ExactVersion, PathBuf> {
    let mut executables = HashMap::new();
    for path in paths {
        if let Ok(version) = ExactVersion::from_path(&path) {
            executables.entry(version).or_insert(path);
        }
    }
    executables
}

pub fn all_executables() -> HashMap<ExactVersion, PathBuf> {
    let paths = flatten_directories(env_path());
    all_executables_in_paths(paths)
}

fn find_executable_in_hashmap(
    requested: RequestedVersion,
    found_executables: &HashMap<ExactVersion, PathBuf>,
) -> Option<PathBuf> {
    let mut iter = found_executables.iter();
    match requested {
        RequestedVersion::Any => iter.max(),
        RequestedVersion::MajorOnly(_) => iter.filter(|pair| pair.0.supports(requested)).max(),
        RequestedVersion::Exact(_, _) => iter.find(|pair| pair.0.supports(requested)),
    }
    .map(|pair| pair.1.clone())
}

pub fn find_executable(requested: RequestedVersion) -> Option<PathBuf> {
    let found_executables = all_executables();
    find_executable_in_hashmap(requested, &found_executables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requestedversion_to_string() {
        assert_eq!(RequestedVersion::Any.to_string(), "Python");
        assert_eq!(RequestedVersion::MajorOnly(3).to_string(), "Python 3");
        assert_eq!(RequestedVersion::Exact(3, 8).to_string(), "Python 3.8");
    }

    #[test]
    fn test_requestedversion_from_str() {
        assert!(RequestedVersion::from_str(".3").is_err());
        assert!(RequestedVersion::from_str("3.").is_err());
        assert!(RequestedVersion::from_str("h").is_err());
        assert!(RequestedVersion::from_str("3.b").is_err());
        assert!(RequestedVersion::from_str("a.7").is_err());
        assert_eq!(RequestedVersion::from_str(""), Ok(RequestedVersion::Any));
        assert_eq!(
            RequestedVersion::from_str("3"),
            Ok(RequestedVersion::MajorOnly(3))
        );
        assert_eq!(
            RequestedVersion::from_str("3.8"),
            Ok(RequestedVersion::Exact(3, 8))
        );
        assert_eq!(
            RequestedVersion::from_str("42.13"),
            Ok(RequestedVersion::Exact(42, 13))
        );
        assert!(RequestedVersion::from_str("3.6.5").is_err());
    }

    #[test]
    fn test_requstedversion_env_var() {
        assert_eq!(
            RequestedVersion::Any.env_var(),
            Some("PY_PYTHON".to_string())
        );
        assert_eq!(
            RequestedVersion::MajorOnly(3).env_var(),
            Some("PY_PYTHON3".to_string())
        );
        assert_eq!(
            RequestedVersion::MajorOnly(42).env_var(),
            Some("PY_PYTHON42".to_string())
        );
        assert!(RequestedVersion::Exact(42, 13).env_var().is_none());
    }

    #[test]
    fn test_requestedversion_from_exactversion() {
        assert_eq!(
            RequestedVersion::from(ExactVersion {
                major: 42,
                minor: 13
            }),
            RequestedVersion::Exact(42, 13)
        );
    }

    #[test]
    fn test_exactversion_to_string() {
        assert_eq!(
            ExactVersion { major: 3, minor: 8 }.to_string(),
            "3.8".to_string()
        );
        assert_eq!(
            ExactVersion {
                major: 42,
                minor: 13
            }
            .to_string(),
            "42.13".to_string()
        );
    }

    #[test]
    fn test_exactversion_from_str() {
        assert_eq!(ExactVersion::from_str(""), Err(Error::DotMissing));
        assert_eq!(ExactVersion::from_str("3"), Err(Error::DotMissing));
        assert_eq!(
            ExactVersion::from_str(".7"),
            Err(Error::ParseVersionComponentError(
                "".parse::<ComponentSize>().unwrap_err()
            ))
        );
        assert_eq!(
            ExactVersion::from_str("3."),
            Err(Error::ParseVersionComponentError(
                "".parse::<ComponentSize>().unwrap_err()
            ))
        );
        assert_eq!(
            ExactVersion::from_str("3.Y"),
            Err(Error::ParseVersionComponentError(
                "Y".parse::<ComponentSize>().unwrap_err()
            ))
        );
        assert_eq!(
            ExactVersion::from_str("X.7"),
            Err(Error::ParseVersionComponentError(
                "X".parse::<ComponentSize>().unwrap_err()
            ))
        );
        assert_eq!(
            ExactVersion::from_str("42.13"),
            Ok(ExactVersion {
                major: 42,
                minor: 13
            })
        );
    }

    #[test]
    fn test_exactversion_from_path() {
        assert_eq!(
            ExactVersion::from_path(&PathBuf::from("/")),
            Err(Error::FileNameMissing)
        );
        assert_eq!(
            ExactVersion::from_path(&PathBuf::from("/notpython")),
            Err(Error::PathFileNameError)
        );
        assert_eq!(
            ExactVersion::from_path(&PathBuf::from("/python3")),
            Err(Error::PathFileNameError)
        );
        assert_eq!(
            ExactVersion::from_path(&PathBuf::from("/pythonX.Y")),
            Err(Error::ParseVersionComponentError(
                "X".parse::<ComponentSize>().unwrap_err()
            ))
        );
        assert_eq!(
            ExactVersion::from_path(&PathBuf::from("/python42.13")),
            Ok(ExactVersion {
                major: 42,
                minor: 13
            })
        );
    }

    #[test]
    fn test_exactversion_supports() {
        let example = ExactVersion { major: 3, minor: 7 };

        assert!(example.supports(RequestedVersion::Any));

        assert!(!example.supports(RequestedVersion::MajorOnly(2)));
        assert!(example.supports(RequestedVersion::MajorOnly(3)));

        assert!(!example.supports(RequestedVersion::Exact(2, 7)));
        assert!(!example.supports(RequestedVersion::Exact(3, 6)));
        assert!(example.supports(RequestedVersion::Exact(3, 7)));
    }

    #[test]
    fn test_all_executables_in_paths() {
        let python27_path = PathBuf::from("/dir1/python2.7");
        let python36_dir1_path = PathBuf::from("/dir1/python3.6");
        let python36_dir2_path = PathBuf::from("/dir2/python3.6");
        let python37_path = PathBuf::from("/dir2/python3.7");
        let files = vec![
            python27_path.to_owned(),
            python36_dir1_path.to_owned(),
            python36_dir2_path,
            python37_path.to_owned(),
        ];

        let executables = all_executables_in_paths(files.into_iter());
        assert_eq!(executables.len(), 3);

        let python27_version = ExactVersion { major: 2, minor: 7 };
        assert!(executables.contains_key(&python27_version));
        assert_eq!(executables.get(&python27_version), Some(&python27_path));

        let python36_version = ExactVersion { major: 3, minor: 6 };
        assert!(executables.contains_key(&python27_version));
        assert_eq!(
            executables.get(&python36_version),
            Some(&python36_dir1_path)
        );

        let python37_version = ExactVersion { major: 3, minor: 7 };
        assert!(executables.contains_key(&python37_version));
        assert_eq!(executables.get(&python37_version), Some(&python37_path));
    }

    #[test]
    fn test_find_executable_in_hashmap() {
        let mut executables = HashMap::new();
        assert_eq!(
            find_executable_in_hashmap(RequestedVersion::Any, &executables),
            None
        );

        let python36_path = PathBuf::from("/python3.6");
        executables.insert(ExactVersion { major: 3, minor: 6 }, python36_path.clone());

        let python37_path = PathBuf::from("/python3.7");
        executables.insert(ExactVersion { major: 3, minor: 7 }, python37_path.clone());

        assert_eq!(
            find_executable_in_hashmap(RequestedVersion::Any, &executables),
            Some(python37_path.clone())
        );

        assert_eq!(
            find_executable_in_hashmap(RequestedVersion::MajorOnly(42), &executables),
            None
        );
        assert_eq!(
            find_executable_in_hashmap(RequestedVersion::MajorOnly(3), &executables),
            Some(python37_path)
        );

        assert_eq!(
            find_executable_in_hashmap(RequestedVersion::Exact(3, 8), &executables),
            None
        );
        assert_eq!(
            find_executable_in_hashmap(RequestedVersion::Exact(3, 6), &executables),
            Some(python36_path)
        );
    }
}

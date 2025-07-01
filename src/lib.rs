use std::str::FromStr;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use bytes::Bytes;
use regex::Regex;
use std::sync::LazyLock;


pub static GITHUB_SPEC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?<project>[^\/]+)\/(?<repository>[^[\+:]]+)(?<branch>\+[^:]+)?:(?<path>[^@]+)@(?<services>.+)$").expect("should be able to compile basic github repo regex")
});

use thiserror::Error;


#[derive(Debug, Error)]
pub enum YammerError {
    #[error("Failed to download file: {0}")]
    Download(#[from] DownloadError),

    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error("Failed to make sense of file source: {0}")]
    UnknownSpec(String)
}


#[derive(Debug, Error)]
pub enum DownloadError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub struct GithubFileSpec<S> {
    pub project: S,
    pub repository: S, 
    pub branch: S,
    pub filepath: S,
}

impl<S> GithubFileSpec<S> {
    pub fn new(project: S, repository: S, branch: S, filepath: S) -> Self {
        Self {
            project,
            repository,
            branch,
            filepath
        }
    }
}


impl<S> GithubFileSpec<S> 
where S: AsRef<str>
{
    pub fn get_url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/refs/heads/{}/{}",
            self.project.as_ref(),
            self.repository.as_ref(),
            self.branch.as_ref(),
            self.filepath.as_ref(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct GithubFileDownloader {
    pub client: reqwest::Client
}

impl GithubFileDownloader {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

impl Default for GithubFileDownloader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DownloadFile for GithubFileDownloader 
{
    type FileSpec = GithubFileSpec<String>;
    async fn download_file(&self, spec: &Self::FileSpec) -> Result<Bytes, YammerError> {
        let url = spec.get_url();

        let response = self.client.get(url).send().await?;
        let response = response.error_for_status()?;
        Ok(response.bytes().await?)
    }
}


#[async_trait]
pub trait DownloadFile {
    type FileSpec: Send + Sync;
    async fn download_file(&self, spec: &Self::FileSpec) -> Result<Bytes, YammerError>;
    async fn download_compose_file(&self, spec: &Self::FileSpec) -> Result<DockerComposeFile, YammerError> {
        let contents = self.download_file(spec).await?;
        Ok(DockerComposeFile::try_from(&contents)?)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerComposeFile {
    pub version: Option<String>,
    pub services: Option<serde_yaml::Mapping>
}

impl TryFrom<&Bytes> for DockerComposeFile {
    type Error = serde_yaml::Error;
    fn try_from(value: &Bytes) -> Result<Self, Self::Error> {
        serde_yaml::from_reader(std::io::Cursor::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct ComposeServiceGithubSpec<S> {
    pub spec: GithubFileSpec<S>,
    pub services: Vec<S>,
}

impl DockerComposeFile {
    pub fn get_service(&self, name: &str) -> Option<&serde_yaml::Mapping> {
        let services = self.services.as_ref()?;
        services.get(name).and_then(|value| value.as_mapping())
    }
}


impl FromStr for ComposeServiceGithubSpec<String> {
    type Err = YammerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some(captures) = GITHUB_SPEC_RE.captures(s) else {
            return Err(YammerError::UnknownSpec("Doesn't match expected regex.".to_string()));
        };
        let Some(project) = captures.name("project").map(|m| m.as_str()) else {
            return Err(YammerError::UnknownSpec("project/user is not specified".to_string()));
        };
        let Some(repository) = captures.name("repository").map(|m| m.as_str()) else {
            return Err(YammerError::UnknownSpec("repository is not specified".to_string()));
        };
        let Some(path) = captures.name("path").map(|m| m.as_str()) else {
            return Err(YammerError::UnknownSpec("path is not specified".to_string()));
        };
        let Some(services_csv) = captures.name("services").map(|m| m.as_str()) else {
            return Err(YammerError::UnknownSpec("no services are specified".to_string()));
        };
        let branch = captures.name("branch").map(|m| {
            let s = m.as_str();
            s.split("+").last().unwrap()
        }).unwrap_or_else(|| "master");

        let spec = GithubFileSpec::new(project.to_string(), repository.to_string(), branch.to_string(), path.to_string());
        let services = services_csv.split(",").map(|s| s.to_owned()).collect();
        Ok(ComposeServiceGithubSpec { spec, services })
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_file_spec_from_str() {
        let service_spec: ComposeServiceGithubSpec<String> = "Data4Democracy/docker-scaffolding+main:docker-compose.yml@postgres".parse().expect("should capture");
        let spec = service_spec.spec;
        assert_eq!(spec.branch, "main");
        assert_eq!(spec.filepath, "docker-compose.yml");
        assert_eq!(spec.project, "Data4Democracy");
        assert_eq!(spec.repository, "docker-scaffolding");
        assert_eq!(service_spec.services, vec!["postgres"]);
    }

    #[test]
    fn test_github_file_spec_from_str_default_branch() {
        let service_spec: ComposeServiceGithubSpec<String> = "Data4Democracy/docker-scaffolding:docker-compose.yml@foo,bar".parse().unwrap();
        let spec = service_spec.spec;
        assert_eq!(spec.branch, "master");
        assert_eq!(spec.filepath, "docker-compose.yml");
        assert_eq!(spec.project, "Data4Democracy");
        assert_eq!(spec.repository, "docker-scaffolding");
        assert_eq!(service_spec.services, vec!["foo", "bar"]);
    }

    #[tokio::test]
    async fn test_download() {
        let service_spec: ComposeServiceGithubSpec<String> = "Data4Democracy/docker-scaffolding:docker-compose.yml@postgres".parse().unwrap();

        let downloader = GithubFileDownloader::new();
        let compose_file = downloader.download_compose_file(&service_spec.spec).await.unwrap();
        let config = compose_file.get_service(&service_spec.services[0]).unwrap();

        let expected = r#"
        build: docker/postgres
        image: postgres"#;
        let expected: serde_yaml::Mapping = serde_yaml::from_str(&expected).unwrap();
        assert_eq!(config, &expected);
    }

}
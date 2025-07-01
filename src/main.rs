use yammer::*;
use clap::Parser;
use std::{collections::HashMap, fs::read_to_string, io::Write, path::PathBuf};

#[derive(Debug, Parser)]
#[clap(
    author,
    version,
)]
/// Scaffold docker compose files by composing them across various compose files over Github repositories.
pub struct Opts {
    /// Any number of compose file spec's (i.e. a DSN to identify a specific service in a docker compose file on some Github repository.)
    /// 
    /// For example, the following DSN represents a subset of the `x-postgres` and `redis` services from [omnivore-app/omnivore](https://github.com/omnivore-app/omnivore/blob/main/docker-compose.yml) file:
    /// `omnivore-app/omnivore+main:docker-compose.yml@redis,x-postgres`
    #[arg(
        value_name = "SERVICE",
        required = true,
    )]
    pub compose_services: Vec<ComposeServiceGithubSpec<String>>,

    /// A path to the docker compose file to merge the composed services into.
    /// If a docker compose file at the destination already exists, then only any
    /// new services are added to it (same names will overwrite the service).
    #[arg(
        short, 
        long, 
        help = "The path to the docker-compose file to merge the services into.",
        default_value = "./docker-compose.yml"
    )]
    pub output: PathBuf
}


#[tokio::main]
async fn main() {
    let opts: Opts = Opts::parse();

    let mut merged = HashMap::<serde_yaml::Value, serde_yaml::Value>::new();
    let downloader = GithubFileDownloader::new();
    let mut version = None;

    for compose_services in opts.compose_services {
        let spec = compose_services.spec;
        let services = compose_services.services;
        match downloader.download_compose_file(&spec).await {
            Ok(compose_file) => {
                let compose_file_version = compose_file.version.clone();
                if version.is_none() && compose_file_version.is_some() {
                    version = Some(compose_file_version.unwrap());
                }

                for service in services {
                    if let Some(service_contents) = compose_file.get_service(&service) {
                        merged.insert(service.into(), serde_yaml::Value::Mapping(service_contents.clone()));
                    }
                }
            },
            Err(err) => {
                eprintln!("failed to download compose file from spec: {err}");
                continue;
            }
        }
    }

    let mut merged_outer: HashMap<serde_yaml::Value, serde_yaml::Value> = HashMap::new();

    let mapping: serde_yaml::Mapping = merged.into_iter().collect();
    merged_outer.insert("services".into(), serde_yaml::Value::Mapping(mapping));
    merged_outer.insert("version".into(), version.unwrap().into());

    let mut all_contents: HashMap<serde_yaml::Value, serde_yaml::Value> = HashMap::default();

    let output_file = opts.output.clone();
    if opts.output.exists() {
        let base_contents = read_to_string(opts.output).unwrap();
        let existing_contents: HashMap<serde_yaml::Value, serde_yaml::Value> = {
            let existing_contents: DockerComposeFile = serde_yaml::from_str(&base_contents).unwrap();
            let existing_services: HashMap<serde_yaml::Value, serde_yaml::Value> = existing_contents.services.map(|svs| svs.into_iter().collect()).unwrap_or_default();
            let mut res = HashMap::default();
            let mapping: serde_yaml::Mapping = existing_services.into_iter().collect();
            res.insert("services".into(), serde_yaml::Value::Mapping(mapping));
            res
        };
        all_contents.extend(existing_contents.into_iter());
    }
    all_contents.extend(merged_outer.into_iter());
    let serialized = serde_yaml::to_string(&all_contents).unwrap();

    let mut file = std::fs::File::create(output_file).unwrap();
    file.write_all(serialized.as_bytes()).unwrap();
}

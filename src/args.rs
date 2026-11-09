use std::{net::IpAddr, str::FromStr};

use crate::common::MigrationType;

pub trait Parseable {
    fn with_docopt(docopt: &docopt::Docopt) -> Self;
}

pub struct Args {
    pub file_path: String,
    pub n_threads: usize,
    pub out_file: String,
    pub wait_headers: bool,
    pub migration_type: Option<MigrationType>,
    pub migration_ip: Option<IpAddr>,
    pub primary_ip: Option<IpAddr>,
    pub fast: bool,
    pub no_progress: bool,
    pub multipath: bool,
}

pub const USAGE: &str = "Usage:
  migration_checker [options] FILE
  migration_checker -h | --help

Options:
  --threads THREADS             Use the given number of thread [default: 100].
  -o --output FILE              Write the result to the given file [default: out.json].
  -w --wait-headers             Wait response header before migration.
  -m --migration-addr ADDRESS   Perform connection migration on the given address.
  -a --address ADDRESS          Use the provide address as primary address.
  -h --help                     Show this screen.
  -t --type TYPE                Change the type of migration checked [default: standard].
  -f --fast                     Shutdown the connection once headers are received.
  --no-progress                 Disable the progress bar.
  -p --multipath                Test multipath QUIC support (advertises multipath in the handshake and reports whether the server negotiates it).
";

impl Parseable for Args {
    fn with_docopt(docopt: &docopt::Docopt) -> Self {
        let args = docopt.parse().unwrap_or_else(|e| e.exit());
        let file_path = args.get_str("FILE").to_string();
        let out_file = match args.get_str("--output") {
            "" => "out.json".to_string(),
            file => file.to_string()
        };
        let n_threads = match args.get_str("--threads") {
            "" => 100,
            val => val.parse().expect("Invalid Number")
        };
        let migration_ip = match args.get_str("--migration-addr") {
            "" => None,
            val => Some(IpAddr::from_str(val).expect("Failed to parse IP"))
        };
        let primary_ip = match args.get_str("--address") {
            "" => None,
            val => Some(IpAddr::from_str(val).expect("Failed to parse IP"))
        };
        let wait_headers = args.get_bool("--wait-headers");
        let migration_type = match args.get_str("--type") {
            "" => Some(MigrationType::Standard),
            "standard" => Some(MigrationType::Standard),
            "passive" => Some(MigrationType::Passive),
            "reuseCID" => Some(MigrationType::ReusedCID),
            "none" => None,
            _ => panic!("Unknown migration type. migrations supported: standard, passive, reuseCID")
        };
        let fast = args.get_bool("--fast");
        let no_progress = args.get_bool("--no-progress");
        let multipath = args.get_bool("--multipath");
        Args {
            file_path,
            n_threads,
            out_file,
            wait_headers,
            migration_type,
            migration_ip,
            primary_ip,
            fast,
            no_progress,
            multipath,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Args {
        let mut v = vec!["migration_checker"];
        v.extend_from_slice(argv);
        let docopt = docopt::Docopt::new(USAGE).unwrap().argv(v);
        Args::with_docopt(&docopt)
    }

    #[test]
    fn defaults() {
        let a = parse(&["targets.csv"]);
        assert_eq!(a.file_path, "targets.csv");
        assert_eq!(a.n_threads, 100);
        assert_eq!(a.out_file, "out.json");
        assert_eq!(a.migration_type, Some(MigrationType::Standard));
        assert!(!a.wait_headers);
        assert!(!a.fast);
        assert!(!a.no_progress);
        assert!(!a.multipath);
        assert!(a.migration_ip.is_none());
        assert!(a.primary_ip.is_none());
    }

    #[test]
    fn multipath_short_flag() {
        assert!(parse(&["-p", "f.csv"]).multipath);
    }

    #[test]
    fn multipath_long_flag() {
        assert!(parse(&["--multipath", "f.csv"]).multipath);
    }

    #[test]
    fn no_progress_flag() {
        assert!(parse(&["--no-progress", "f.csv"]).no_progress);
    }

    #[test]
    fn fast_and_wait_headers_flags() {
        let a = parse(&["-f", "-w", "f.csv"]);
        assert!(a.fast);
        assert!(a.wait_headers);
        let a = parse(&["--fast", "--wait-headers", "f.csv"]);
        assert!(a.fast);
        assert!(a.wait_headers);
    }

    #[test]
    fn migration_types() {
        assert_eq!(parse(&["-t", "standard", "f"]).migration_type, Some(MigrationType::Standard));
        assert_eq!(parse(&["-t", "passive", "f"]).migration_type, Some(MigrationType::Passive));
        assert_eq!(parse(&["--type", "reuseCID", "f"]).migration_type, Some(MigrationType::ReusedCID));
        assert_eq!(parse(&["-t", "none", "f"]).migration_type, None);
    }

    #[test]
    fn addresses_v4_and_v6() {
        let a = parse(&["-a", "192.0.2.1", "-m", "2001:db8::1", "f"]);
        assert_eq!(a.primary_ip, Some("192.0.2.1".parse().unwrap()));
        assert_eq!(a.migration_ip, Some("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn threads_and_output() {
        let a = parse(&["--threads", "7", "-o", "res.json", "f"]);
        assert_eq!(a.n_threads, 7);
        assert_eq!(a.out_file, "res.json");
    }
}

use std::net::IpAddr;
use std::time::Instant;

use crate::common::{MigrationStatus, MigrationType, Target, TestResult};
use crate::quiche_client::*;


pub fn test_migration(target: &Target, primary_ip: Option<IpAddr>, migration_ip: Option<IpAddr>, wait_headers: bool, migration_type: Option<MigrationType>, fast: bool, multipath: bool) -> TestResult{
    let mut result: TestResult = TestResult::from_target(target);

    let mut client = match QuicheClient::init(target, primary_ip, migration_ip, multipath){
        Ok(client) => client,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };

    let start_time = Instant::now();
    match client.connect(){
        Ok(()) => (),
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    }

    result.peer_addr = Some(client.peer_addr);
    result.handshake_done = true;

    if multipath{
        result.multipath_supported = client.multipath_supported();
    }
    result.peer_disable_active_migration = client.peer_disables_active_migration();

    if !wait_headers && migration_type.is_some(){
        let status = match client.probe_path_and_migrate(migration_type.unwrap()){
            Ok(status) => status,
            Err(e) => {
                result.duration = Some(start_time.elapsed());
                result.error = Some(e);
                return result;
            }
        };
        if status != MigrationStatus::Failed{
            result.migrated = true;
        }
    }

    let response_headers = match client.perform_request_with_redirect(None, &mut None, 5, wait_headers, migration_type, !fast){
        Ok(hdrs) => hdrs,
        Err(e) => {
            result.duration = Some(start_time.elapsed());
            result.error = Some(e);
            return result;
        }
    }; 
    result.duration = Some(start_time.elapsed());

    result.stats = client.get_stats();
    result.response_headers = response_headers;

    if client.migration_status.is_some(){
        result.migrated = true;
        result.migration_status = client.migration_status.clone();
    }



    if !fast{
        match client.close() {
            Ok(()) => (),
            Err(e) => {
                result.error = Some(e);
                return result;
            }
        };
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Target;
    use std::net::{IpAddr, Ipv4Addr};

    fn target_for(port: u16) -> Target {
        Target {
            domain: format!("https://127.0.0.1:{}", port),
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
        }
    }

    #[test]
    fn multipath_is_reported_when_server_supports_it() {
        let addr = crate::test_server::spawn(true);
        let target = target_for(addr.port());

        let result = test_migration(&target, None, None, false, None, true, true);

        assert!(result.handshake_done, "handshake failed: {:?}", result.error);
        assert_eq!(result.multipath_supported, Some(true));
    }

    #[test]
    fn multipath_is_not_reported_when_server_lacks_support() {
        let addr = crate::test_server::spawn(false);
        let target = target_for(addr.port());

        let result = test_migration(&target, None, None, false, None, true, true);

        assert!(result.handshake_done, "handshake failed: {:?}", result.error);
        assert_eq!(result.multipath_supported, Some(false));
    }
}
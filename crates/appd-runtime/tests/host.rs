use std::path::Path;

use appd_runtime::host::{bundle_path, work_dir_in_apple_resources, work_dir_next_to_exe};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn resolves_desktop_work_dir_next_to_executable() -> TestResult {
    let work_dir = work_dir_next_to_exe(Path::new("/opt/demo/demo-app"))?;

    assert_eq!(work_dir, Path::new("/opt/demo/app"));
    Ok(())
}

#[test]
fn resolves_apple_work_dir_inside_bundle_resources() {
    let work_dir = work_dir_in_apple_resources(Path::new("/Demo.app/Contents/Resources"));

    assert_eq!(work_dir, Path::new("/Demo.app/Contents/Resources/app"));
}

#[test]
fn resolves_bundle_path_inside_work_dir() {
    let path = bundle_path(Path::new("/opt/demo/app"));

    assert_eq!(path, Path::new("/opt/demo/app/worker.bundle"));
}

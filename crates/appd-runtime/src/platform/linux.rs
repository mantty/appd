//! Linux `GTK4` + `WebKitGTK 6` runtime shell.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;

use webkit6::gio;
use webkit6::gtk::{self, glib};
use webkit6::prelude::*;
use webkit6::{Credential, CredentialPersistence, NetworkSession, WebView};

use crate::certs::CertificateBundle;
use crate::host::work_dir_next_to_current_exe_or_default;

type SharedCertificates = Arc<RwLock<Option<CertificateBundle>>>;

/// Start the Linux runtime shell.
pub fn run() -> ! {
    let app = gtk::Application::builder()
        .application_id("com.appd.runtime")
        .build();
    let certificates = Arc::new(RwLock::new(None));
    app.connect_activate(move |app| activate(app, Arc::clone(&certificates)));
    let status = app.run();
    std::process::exit(status.into());
}

fn activate(app: &gtk::Application, certificates: SharedCertificates) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("appd")
        .default_width(1024)
        .default_height(768)
        .build();

    let network_session = NetworkSession::new_ephemeral();
    let webview = WebView::builder()
        .network_session(&network_session)
        .vexpand(true)
        .hexpand(true)
        .build();

    connect_tls_handlers(&webview, &network_session, Arc::clone(&certificates));

    #[allow(deprecated)]
    let (port_sender, port_receiver) = glib::MainContext::channel(glib::Priority::default());
    port_receiver.attach(None, {
        let webview = webview.clone();
        move |port| {
            webview.load_uri(&crate::frontend_url(port, false));
            glib::ControlFlow::Break
        }
    });

    window.set_child(Some(&webview));
    window.present();

    let work_dir = work_dir_next_to_current_exe_or_default();
    let _ = thread::Builder::new()
        .name("appd-workerd-init".to_owned())
        .spawn(move || start_runtime(work_dir, certificates, port_sender));
}

fn connect_tls_handlers(
    webview: &WebView,
    network_session: &NetworkSession,
    certificates: SharedCertificates,
) {
    let session = network_session.clone();
    webview.connect_load_failed_with_tls_errors(move |webview, failing_uri, certificate, _| {
        session.allow_tls_certificate_for_host(certificate, "localhost");
        webview.load_uri(failing_uri);
        true
    });

    webview.connect_authenticate(move |_, request| {
        let Some(certs) = certificates.read().ok().and_then(|guard| guard.clone()) else {
            return false;
        };
        let Ok(tls_certificate) =
            gio::TlsCertificate::from_pkcs12(&certs.client_p12_der, Some(&certs.p12_password))
        else {
            return false;
        };
        let credential =
            Credential::for_certificate(&tls_certificate, CredentialPersistence::ForSession);
        request.authenticate(Some(&credential));
        true
    });
}

#[cfg(feature = "workerd-ffi")]
fn start_runtime(
    work_dir: PathBuf,
    certificates: SharedCertificates,
    port_sender: glib::Sender<u16>,
) {
    match crate::host::start_workerd_bridge(&work_dir) {
        Ok(runtime) => {
            if let Ok(mut guard) = certificates.write() {
                *guard = Some(runtime.certificates.clone());
            }
            let port = runtime.port;
            let _ = port_sender.send(port);
            let _ = runtime.join();
        }
        Err(error) => {
            eprintln!("appd runtime startup failed: {error:#}");
        }
    }
}

#[cfg(not(feature = "workerd-ffi"))]
fn start_runtime(_: PathBuf, _: SharedCertificates, _: glib::Sender<u16>) {
    eprintln!("appd runtime was built without the workerd-ffi feature");
}

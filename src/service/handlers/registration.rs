use crate::ipc::{Request, Response};
use crate::service::ManagedClient;
use std::collections::HashMap;

pub async fn handle_register(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = super::get_account(req, "register", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    *mc.should_register.lock().await = true;
    let client = mc.client.lock().await;
    match client.register().await {
        Ok(true) => Response::ok(&format!("'{}' registered", req.account.as_deref().unwrap())),
        Ok(false) => Response::fail(&format!(
            "'{}' registration failed",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

pub async fn handle_unregister(
    req: &Request,
    clients: &HashMap<String, ManagedClient>,
) -> Response {
    let account_name = super::get_account(req, "unregister", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    *mc.should_register.lock().await = false;
    let client = mc.client.lock().await;
    match client.unregister().await {
        Ok(true) => Response::ok(&format!(
            "'{}' unregistered",
            req.account.as_deref().unwrap()
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' unregistration failed",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

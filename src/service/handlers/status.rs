use crate::ipc::Response;
use crate::service::ManagedClient;
use std::collections::HashMap;

pub async fn handle_status(clients: &HashMap<String, ManagedClient>) -> Response {
    let mut lines = vec![];
    for (name, mc) in clients {
        let client = mc.client.lock().await;
        let state = if client.in_call { "in call" } else { "idle" };
        lines.push(format!(
            "  {}: {}@{} bound={} {}",
            name, client.username, client.domain, client.local_addr, state
        ));
    }
    Response::ok(&format!("Accounts:\n{}", lines.join("\n")))
}

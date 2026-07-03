pub mod mcp_client;

#[cfg(test)]
mod tests {
    #[test]
    fn plugin_module_is_exported() {
        let _ = std::mem::size_of::<crate::plugins::mcp_client::ExternalMcpServer>();
    }
}

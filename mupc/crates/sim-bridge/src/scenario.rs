/// 5 场景模式参数常量。
/// 与 MUPC-AI2 `mupc_env/constants.py` 和 PRD §2.3 对齐。

pub const SCENARIOS: &[&str] = &["MODE-01", "MODE-02", "MODE-03", "MODE-04", "MODE-05"];

pub fn validate_scenario(name: &str) -> Result<(), String> {
    if SCENARIOS.contains(&name) {
        Ok(())
    } else {
        Err(format!(
            "无效场景: {}。有效值: {}",
            name,
            SCENARIOS.join(", ")
        ))
    }
}

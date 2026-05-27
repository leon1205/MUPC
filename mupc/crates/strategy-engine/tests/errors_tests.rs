use mupc_strategy_engine::StrategyError;

#[test]
fn test_execution_failed() {
    let err = StrategyError::ExecutionFailed("AI model timeout".to_string());
    assert_eq!(err.to_string(), "策略执行失败: AI model timeout");
}

#[test]
fn test_model_error() {
    let err = StrategyError::ModelError("invalid input shape".to_string());
    assert_eq!(err.to_string(), "AI 模型错误: invalid input shape");
}

#[test]
fn test_config_error() {
    let err = StrategyError::ConfigError("missing threshold".to_string());
    assert_eq!(err.to_string(), "配置错误: missing threshold");
}

#[test]
fn test_debug_format() {
    let err = StrategyError::ExecutionFailed("test".to_string());
    let debug = format!("{:?}", err);
    assert!(debug.contains("ExecutionFailed"));
}
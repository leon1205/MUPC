//! 南向命令发送器测试

use crate::south_command_sender::{
    get_dispatcher, set_dispatcher, LoadSheddingCommand, MockSouthCommandSender, PvLimitCommand,
    SouthCommandDispatcher, SouthCommandSender, SouthCommandType, SouthSendResult,
};

#[tokio::test]
async fn test_pv_limit_command() {
    let sender = MockSouthCommandSender::new();
    let cmd = PvLimitCommand {
        device_id: "pv_inverter_001".to_string(),
        limit_ratio: 0.75,
        priority: 1,
    };
    let result = sender.send_pv_limit(cmd).await;
    assert!(result.success);
    assert_eq!(result.device_id, "pv_inverter_001");
    assert!(matches!(result.command_type, SouthCommandType::PvLimit));
    assert_eq!(sender.pv_limit_sent_count(), 1);
}

#[tokio::test]
async fn test_load_shedding_command() {
    let sender = MockSouthCommandSender::new();
    let cmd = LoadSheddingCommand {
        device_id: "load_ctrl_001".to_string(),
        power_kw: 25.0,
        priority: 2,
    };
    let result = sender.send_load_shedding(cmd).await;
    assert!(result.success);
    assert_eq!(result.device_id, "load_ctrl_001");
    assert!(matches!(
        result.command_type,
        SouthCommandType::LoadShedding
    ));
    assert_eq!(sender.load_shedding_sent_count(), 1);
}

#[tokio::test]
async fn test_dispatcher_pv_limit() {
    let dispatcher = SouthCommandDispatcher::with_mock("pv_001", "load_001");
    let result = dispatcher.dispatch_pv_limit(0.8, 1).await;
    assert!(result.success);
    assert_eq!(result.device_id, "pv_001");
}

#[tokio::test]
async fn test_dispatcher_load_shedding() {
    let dispatcher = SouthCommandDispatcher::with_mock("pv_001", "load_001");
    let result = dispatcher.dispatch_load_shedding(30.0, 2).await;
    assert!(result.success);
    assert_eq!(result.device_id, "load_001");
}

#[tokio::test]
async fn test_global_dispatcher() {
    let dispatcher = SouthCommandDispatcher::with_mock("global_pv", "global_load");
    set_dispatcher(std::sync::Arc::new(dispatcher));
    let result = get_dispatcher();
    assert!(result.is_some());
}

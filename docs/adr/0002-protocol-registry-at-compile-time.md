# ADR-0002：协议注册从运行时反射改为编译期收集

- 状态：已接受
- 日期：2026-10-24

## 背景

原框架 `MessageHandlerManager.loadHandler` 扫描包 `bf.game.server.message.handler`，
用 `@LabelMessageHandler` + `@LabelMessageMethod` 标注，cmdId 由
`Integer.parseInt(类名最后一个 _ 之后的部分)` 推出。问题：

1. 启动才有结果 —— 配置里有、代码里没有的 cmdId 到运行时才暴露
2. 类名即协议号，重命名会静默改协议
3. 反射扫描在 5000 类规模下启动开销明显

## 决定

用 `inventory` crate 在**编译期**收集 `Box<dyn ErasedHandler>`，
`HandlerTable` 是只读表，启动零成本。

## 后果

- 好的：cmdId 冲突在链接期就 panic；未注册的 cmdId 返回 `Ok(None)` 且记 debug 日志
- 好的：`HandlerTable::empty()` 可用于单测，不需要启动
- 坏的：`inventory` 依赖链接器，需要在 `main` 里 `inventory::submit!` 或宏展开
- 兼容：`free_login_cmds` 白名单与原服务端逐位相同 `[0,3,4,5,6,7,11,98]`

# TODO

[返回](./README.md)

当前主线：frontend / 设备线程已落地（`src/frontend/`），下一步补 BLE、断连处理和完整 CLI。命令实现状态见 [cmd-impl-status.md](./cmd-impl-status.md)。

## 进行中

- [x] `varint.rs` 的 `DekuReader` 按 `VarintMode` 解码
  - 之前四个类型都走 Auto 的 1/2 字节逻辑：`VarFixed3` 丢 `lo`，`VarC0Be16` 把 `hi` 当低字节
  - 现在按模式决定读取长度，再复用 `VarintMode::decode`
  - 新增 `deku_reader_uses_mode` 测试；`cargo test --lib`: 77 passed
  - 顺手删了未使用的 `use std::ops::Range;`

- [x] `settings.rs` 的取值空间统一
  - `SpeedSetting` / `DarknessSetting` 显式 `Min = 0`，discriminant = `v2`/`v1` 线上值
  - `TryFrom<u8>` 改成线上值（`0..=4` / `0..=14`）
  - 新增 `user_value` / `from_user`（用户面 `1..=5` / `1..=15`）
  - 新增 `to_wire(mv)` / `from_wire(v, mv)` 处理 `v0` 的 `+1`
  - `FromStr` 继续按用户面解析；加 `PartialEq, Eq`
  - 5 个用例；`cargo test --lib`: 82 passed

- [x] P0 响应层基础设施（未提交）
  - `src/command/mod.rs` 加 `pub mod resp;`
  - `src/command/resp/mod.rs`：`RespTrait` / `ParseRespError` / `decode_resp`
  - `decode_resp` 用一个 `DekuContainerRead` 模板解析，并要求完整消费 payload；
    magic 缺失落到 `Decode`，多余字节落到 `TrailingBytes`
  - 4 个用例；`cargo test --lib`: 86 passed

- [x] P1 darkness 样板（未提交）
  - `resp_template/darkness.rs`：`_tail` 改为 `Vec<u8>` + `read_all`，3 个字节级用例
  - `resp_template/mod.rs`：新增 `test_toolkits`（`hexdec` / `eq` / `dec_tl`）
  - `resp/darkness.rs`：`Darkness { darkness }`，用 `DarknessSetting::from_wire` 统一
    `v2`/`v1`（`0..=14`）与 `v0`（`1..=15`）
  - `cmd-impl-status.md`：`0x43` resp 列改为 `DarknessV2V1V0Tl`
  - 7 个用例；`cargo test --lib`: 93 passed

- [x] P2 `0x42` / `0x44` / `0x45`
  - `resp_template/speed.rs`：`1f 44 <speed>` + `read_all` 尾部（DP305P 载荷 12 字节）
  - `resp/speed.rs`：`Speed { speed }`，走 `SpeedSetting::from_wire`
  - `resp_template/gap_type.rs`：`1f 42 <gap_type>` + `read_all` 尾部
  - `resp/gap_type.rs`：`GapType { gap_type }`，`GapTypeSetting::from_wire`（全机型一致）
  - `settings.rs`：`GapTypeSetting` 补 `PartialEq, Eq` / `TryFrom<u8>` / `to_wire` / `from_wire`
  - `resp_template/gap_length.rs`：只收原始字节，不做变长解码
  - `resp/gap_length.rs`：按 `raw.len()` 选 `Auto` / `Fixed2` / `Fixed3`，长度越界报
    `BadLength`，缺 `0xC0` 标志报 `InvalidValue`
  - `cmd-impl-status.md`：`0x42` / `0x44` / `0x45` resp 列补类型名
  - `protocol.md`：补 `0x45` 响应按长度解码的说明
  - 24 个用例；`cargo test --lib`: 117 passed

- [x] P2 req domain 补齐
  - 新建 `req/darkness.rs` / `req/speed.rs` / `req/gap_type.rs` / `req/gap_length.rs`
  - 结构与 `req::EndPage` / `req::FeedLine` 一致：选 `EnvelopeKind` → 模板 `to_bytes`
  - 设置值统一通过 `settings.rs` 的 `to_wire` 处理版本差异
  - `GapLength` 只在 `v2` 发送，`v1`/`v0` 返回 `PackReqError::UnsupportedUsage`

- [x] `status.rs` 重写
  - `PrinterErrorCode` 改为 `PrinterStatus`，按线上值映射 25 个状态码
  - 补 `to_wire` / `from_wire` / `TryFrom<u8>` / `is_printable` / `is_error`
  - 修正 `36 = RibbonCanOpened`（原 `TphOpened` 有误）
  - `print-status.md` 同步为完整状态表和实测帧
  - 3 个用例；`cargo test --lib`: 120 passed

- [x] `0x70` req / resp
  - `req_template/status.rs`：`StatusV2V1V0Tl`，空载荷（`1f 70`）
  - `req/status.rs`：`Status {}`，按机型选 `EnvelopeKind`
  - `resp_template/status.rs`：`StatusV2V1V0Tl`，`状态码:1 + PageKey:u16BE + read_all 尾部`
  - `resp/status.rs`：`Status { status, page_key, tail }`，状态码走 `PrinterStatus::from_wire`
  - DP27P 实测帧用作模板和 domain 用例
  - `cmd-impl-status.md`：`0x70` req / resp 列改为 `StatusV2V1V0Tl`
  - `protocol.md` / `print-status.md`：补 PageKey 位置和响应模板链接
  - 9 个用例；`cargo test --lib`: 129 passed

- [x] RLE 编码 + req 层
  - `rle.rs`：`RleBitStream`（4/5/6 位，X/D）和 `RleByteStream`（`0x29`），尾部白色游程不编码
  - 游程拆分用 DP 求最少符号；长游程先取最大码本值再查表
  - `planner/print.rs`：`RleMode` 非 `Off` 时编码，X/D 取小，必须比普通行小才采用，否则回退 `Line`
  - `req/print_line_rle.rs`：`PrintLineRle` + `RleKind`，只在 `v2` 发送，`repeats` 用 `0x2e` 展开；`v1` / `v0` 返回 `UnsupportedUsage`
  - 新增编码器 roundtrip 测试；`cargo test --lib`: 300 passed

- [x] Transport trait + USB 实现
  - `backend/mod.rs`：`Transport`（`write` / `read_chunk` / `reset`）、`WriteOutcome`、`ReadStatus`、`TransportError`
  - `write` 允许返回 `Blocked`，给 BLE 的 `ff03` 信用留口子，传输层不阻塞等读
  - `backend/usb.rs`：`UsbTransport` / `UsbSelector`（设备名 / VID+PID / Auto），端点发现、声明接口、复用 `planner::usb` 报告切片
  - `reset` 后等 1 秒重新枚举
  - 对象安全用例；`cargo test --lib`: 313 passed

- [x] `test_usb` 换用 `UsbTransport` + `FlowPlanner`
  - 删除自己维护的 `USBSelector` / 端点发现 / `write_cmd_area`，改用 `backend::usb::UsbTransport`
  - 新增 `print_bitmap_job`：`StartPage` + `TotalLines` + 按预算分块 + `EndPage`
  - `--phase print` 时打一张测试图，默认只跑 `device-info` 避免误走纸
  - `cargo check --bin test_usb` / clippy 干净

- [x] RLE 帧改成 `ManualPack`（DP27P 实测定位）
  - 现象：`RleMode::Byte` 图形右移、左侧拖影；`Off` 正常
  - 原因：`PrintLineRle` 用 `AutoPackWithChecksum`，`Assembler` 在协议字段 `<RleByteLen>` 前又补了一层 `<len>`；设备把 `RleByteLen` 当成第一个 RLE 数据字节
  - 修复：`PrintLineRle` 改用 `ManualPack`，模板保留 `<RleByteLen>` / `<符号数>`，校验和手动追加
  - 复测：字节级 `0x29` 正常；位级 `0x2c` / `0x3c` 也正常
  - `0x3a`（RLE4X）仍空白；X / D 混用仍会断层，真机用 `Bits5XOnly` / `Bits6XOnly`
  - `cargo test --lib`: 313 passed

- [x] 位级 RLE 的 X / D 混用问题（DP27P 实测）
  - `Bits4` / `Bits5`（自动 X/D）图像中间出现周期性断层；`Bits5XOnly` 打印正常
  - 位流本身自洽，例如 `1f 2c 04 ff ff c0` 解码正好是 384 个黑点
  - 结论：DP27P 不把 X 行写进 D 的“上一行”基准，X / D 不能混用；`RleMode::Bits5` 暂时不要用于真机
  - 已加 `RleMode::Bits4XOnly` / `Bits5XOnly` / `Bits6XOnly`，对应 `--rle bits4x|bits5x|bits6x`
- [x] `0x3a`（RLE4X）在 DP27P 上打不出内容
  - `bits4x` 日志确认 payload 正确：`1f 3a 06 ff ff ae 13`
    - `FF FF AE` = `1111 1111 1111 1111 1010 1110` → 88×4 + 3 + 29 = 384 点
    - 符号顺序由 DP 选出（3 后 29），总长与行宽一致
  - 命令格式和位流都没有问题，因此判断为 DP27P 固件不认 `0x3a`，或 RLE4 码本与 5 位不同
  - `0x3c`（RLE6X）随后复测正常
  - 目前可用的位级方案：`Bits5XOnly`（`0x2c`）/ `Bits6XOnly`（`0x3c`）

- [x] 长位图自动拆页
  - `0x26 TotalLines` 在 v2 上的上限是 `16383`，`print_bitmap` 只描述一页
  - 新增 `planner::print::print_bitmap_pages` / `PrintPage`：按 `MAX_LINES_PER_PAGE`
    切页，每页第一行按全白基准，D 模式不跨页引用上一页
  - `test_usb::print_bitmap_job` 逐页发 `StartPage` / `TotalLines` / 行命令 / `EndPage`，
    `page_idx` 递增，页间保持 `Continuous`；`FlowPlanner` 每页新建
  - `--rows` 可覆盖测试图行数，用来验证多页
  - 多页在 DP27P 上的纸面行为（是否连续、有无额外间距）待实测
  - `cargo test --lib`: 318 passed

- [x] 长页流控死循环（`TotalLines = 16383`）
  - 现象：日志每隔 50 ms 发一次 `1f 70 ... 1f 77 ...`，预算一直是 24500，但从不发送行数据
  - 原因：`FlowPlanner::step` 用「`budget < remaining`」判断要查询。长页的 `remaining`
    永远大于单次预算，于是拿到预算后立刻又进入查询分支
  - 修复：改成「当前预算花完（`budget == 0`）再查 `0x77`」；补回归测试
    `spends_budget_before_querying_again`
  - `flow-control.md` 的发送循环同步改成先发完当前预算

- [x] 等打印真正完成再退出
  - 现象：`EndPage` 之后程序立即结束，纸还在走，USB 句柄一释放打印就中断
  - `EndPage` 只是把数据交给固件，电机是异步驱动的
  - 新增 `Device::wait_for_print_done`：轮询 `0x70`
    - `Printing` / `Rotating` / `PageNotReady` 视为还在忙
    - 连续 2 次 `Printable` / `NoJob` 才认为完成
    - 短任务用 1 秒启动宽限期兜底，避免在固件真正启动前误判
    - `0x0c` 及以上错误状态立即返回
  - 只在最后一个 `EndPage` 后调用；行数据中间不要插 `0x70`
  - 默认超时 10 分钟

## 规划：frontend / 设备线程 / transport

目标：把「命令怎么编码」和「字节怎么送到设备」拆开。调用方只面对 `frontend`，
不关心 USB/BLE，也不直接碰 `Assembler`。

```text
用户代码
  │  同步或 async 调用
  ▼
frontend（选择后端 / 连接参数 / ModelVersion）
  │  mpsc::Sender<Request>
  ▼
设备 worker 线程（每个设备一个）
  │  编码 + 写 transport + 读响应
  ▼
transport（USB HID / BLE 串口透传）
```

- `frontend`
  - 入口：选择 backend（USB / BLE）+ 连接参数 + `ModelVersion`
  - 结构体持有到 worker 的 channel，不直接持有 USB/BLE 句柄
  - 对外方法返回 `Result<T, FrontendError>`
  - 设备断开后：worker 把状态置为不可用，后续调用立即返回错误，不再排队
- 设备 worker
  - 从 channel 取请求，按顺序编码发送
  - 需要响应的请求进 FIFO，每项带一个 `oneshot` 和响应解码器
  - 响应按到达顺序交给队首；半条帧留在缓冲区，等下次数据继续解析
  - 写失败 / 读线程退出：清空 FIFO，所有等待者收到 `DeviceUnavailable`
  - 不做业务判断；流控计划用 `planner::flow`，重试策略放上层
- `transport`
  - 只负责字节读写，不解析命令
  - USB：HID Interrupt + `planner::usb::encode_reports` / `decode_report`
  - BLE：`ff02` 写、`ff01` 响应、`ff03` 的 `01 01` 信用
- 待定
  - 对外 API 用同步还是 async（tokio 已经在依赖里，默认 async？）
  - frontend 是否用 `Disconnected` / `Connected` 类型状态区分
  - 混合响应的分派方式：每请求自带 decoder，还是中央 `Resp` enum
  - 大量无响应打印命令是否单独走批量队列，避免和查询命令抢 FIFO
- 倾向
  - worker 只做「切帧」，把原始 payload 交回 frontend；frontend 的泛型方法再
    `T::parse`。这样不用把 `RespTrait` 擦成 `Any`，也避免跨线程传泛型闭包。
  - 为此需要给 `Assembler` 暴露一个返回 `(Vec<u8>, consumed)` 的公开切帧接口。

- [x] 定义 `Transport` trait（write / read / reset）
- [x] `Assembler` 暴露切帧接口：`decode_frame -> Option<(Vec<u8>, consumed)>`
- [x] USB transport 接 `planner::usb`
- [x] 定义 `frontend::Device` 对外接口和错误类型
- [x] 设备 worker 线程 + 发送 FIFO + 原始 payload 响应队列
- [ ] 严格请求 / 响应配对（当前按调用顺序取，允许换类型重试）
- [ ] BLE transport（`btleplug` + `ff03` 信用）
- [ ] 断连 / 写失败时让所有 pending 请求返回错误
- [ ] 示例：连接 → 读 DPI / 宽度 → 打印一张图

## resp 层约定

- `payload` = `命令组 + 命令类型 + 数据`，不含长度字段和校验和，与 `Envelope.payload` 对称
- 模板保留 `#[deku(magic = ...)]`，头校验交给 deku 的 `DekuRead`；`resp/` 层不再存 `HEADER`
- `RespTrait::parse(payload, mv)`：版本差异（如 v0 的 `darkness + 1`）在 `resp/` 的 domain 层归一化
- 模板只解析**一条**响应；多条响应由帧层按长度字段切分
- 长度不固定的尾部用 `#[deku(read_all)] _tail: Vec<u8>`
- `0x45` 这类 1/2/3 字节变长字段按 `payload.len()` 用 `VarintMode::decode` 选解码器，不用 `VarFixed3` 字段
- 设置类枚举的 discriminant = `v2`/`v1` 线上值；用户面值走 `user_value`/`from_user`，跨版本走 `to_wire`/`from_wire`
- 中央 `Resp` enum 暂不做；调用方静态指定响应类型

## 阶段

### P0 基础设施

- [x] `src/command/mod.rs` 加 `pub mod resp;`
- [x] 新建 `src/command/resp/mod.rs`
  - `RespTrait`
  - `ParseRespError`（`Decode` / `TrailingBytes` / `InvalidValue` / `BadLength`）
  - `decode_resp`（要求模板完整消费 payload；4 个用例）
- [x] `resp_template/mod.rs` 加 `test_toolkits`（`hexdec` / `eq` / `dec_tl`）
- [x] `settings.rs`：`Min = 0`，discriminant = `v2`/`v1` 线上值，补 `user_value` / `from_user` / `to_wire` / `from_wire`

### P1 darkness 样板

- [x] `resp_template/darkness.rs`：`_tail` 改成 `Vec<u8>` + `read_all`，补字节级用例
- [x] 新建 `resp/darkness.rs`：`Darkness { darkness: DarknessSetting }`
  - v2/v1：原始值 `0..=14` → `+1`
  - v0：原始值 `1..=15` → 直接使用
- [x] `cmd-impl-status.md`：`0x43` resp 列改成 `DarknessV2V1V0Tl`

### P2 固定长度查询批（14）

- [x] `0x42` 纸张类型
- [x] `0x44` 速度
- [x] `0x45` 纸间隔（1/2/3 字节，按长度解码）
- [x] `0x42` / `0x43` / `0x44` / `0x45` 的 req domain（`src/command/req/`）
  - `Darkness` / `Speed` 分别用 `DarknessSetting::to_wire(mv)` / `SpeedSetting::to_wire(mv)`
  - `GapType` 用 `GapTypeSetting::to_wire()`（全机型一致）
  - `GapLength` 校验 `50..=4194303`，`v1`/`v0` 返回 `UnsupportedUsage`
- [ ] `0x48` 自动关机（读回 1B；写 1/2B BE）
- [ ] `0x52` 支持纸张类型（4B：`02 03 04 00`）
- [ ] `0x57` 支持电机模式（1B：`0C`）
- [ ] `0x59` 支持语言（2B：`61 01`，语义待确认）
- [ ] `0x5b`（2B：`00 00`，语义待确认）
- [ ] `0x66` 纸张类型（DP27P USB 上裸读 / v1 校验 / 带参数多变体均无响应）
- [x] `0x71` DPI（u16BE）
- [x] `0x73` 统计（4 × u32BE）
- [ ] `0x7a` 硬件版本（5B：`26 01 04 01 04`）
- [x] `0x7f` 设备信息（DP27P 回 16B 全 `00`；参数、激活状态都不影响，语义未确认，原样保留）
- [x] `0x80` 激活 / 结束 `> 0x80` 命令（`0x7f` / `0x00` / `0x49`）

> 以上未勾选项的字节布局都在 DP27P（软件版本 `3.1.20230620`，USB）上实测过；
> `0x66` 除外，多种读取变体均无响应。

### P3 变长 / 字符串批（6）

- [x] `0x70` 打印机状态（状态码 1B + `PageKey` u16BE + tail）
- [x] `0x72` 打印宽度 / 纸宽（u16BE ×2 + tail）
- [x] `0x75` 制造商（NUL 结尾字符串，GBK → `Box<str>`）
- [x] `0x79` 设备名（NUL 结尾字符串 → `Box<str>`）
- [x] `0x7c` 软件版本（NUL 结尾字符串 → `Box<str>`）
- [x] `0x7d` 蓝牙 MAC（`0x10 <6B>` + `0x14 <6B>`）
- [x] GBK 方案：`encoding_rs`；`resp_template` 保留 `Vec<u8>`，`resp` 输出 `Box<str>`
- [x] `status.rs` 重写为 `PrinterStatus`（25 个线上值 + `TryFrom` / `to_wire` / `from_wire` / `is_printable` / `is_error`）

### P4 流控（2）

- [x] `0x77`：`size: VarAuto` + `status: i32 LE` + `syncToken: i16 LE` + tail
- [x] `0x7b`：`size: VarAuto`（只有固件上报，主机不发请求）
- [x] 用 [flow-control.md](./flow-control.md) 第 3 节示例做用例

### P5 帧层

- [x] `src/planner/assembler.rs`：`Assembler::encode` / `decode_one`（命令头 → 变长长度 → data → 校验和）
- [x] `decode_one(buf) -> Option<(resp, consumed)>`：尾部半条返回 `None`，调用方保留缓冲区
- [x] 用例：两条拼接、半条、坏校验、`0x88` 固定校验、2 字节长度字段
- [x] USB `1e <len>` 入口：`src/planner/usb.rs`（封包 / 解包 / 64 字节 HID 报告切片）

### P6 读请求补齐

- [x] `0x70` / `0x71` / `0x72` / `0x73` / `0x75` / `0x77` / `0x79` / `0x7c` / `0x7d` 的读取请求
- [x] `0x7b` 只有固件上报，没有请求
- [ ] `0x52` / `0x57` / `0x59` / `0x66` 的读取请求（等响应格式确认）
- [ ] 决定用 `ReadRequest { group, type_ }` 还是每命令一个空结构体

### P7 后端集成

> 具体接口拆分见上面「规划：frontend / 设备线程 / transport」。

- [x] `lib.rs` 恢复 `pub mod backend;`
- [ ] IN 线程用 `Assembler::decode_one` 替换旧的 `Command::parse_device_command`
- [ ] 队列里放单条 raw payload，调用方用 `RespTrait::parse` 解码
- [ ] 请求/响应 1:1 FIFO 配对
- [ ] 连接后确定 `ModelVersion`：目前只能由调用方指定；SDK `h=0/16/303` 的来源还没确认
- [ ] BLE 后端：`ff02` 写入、`ff01` 响应、`ff03` 的 `01 01` 信用流控（见 [issue #3](https://github.com/sb-child/dz-print/issues/3)）
- [ ] 端到端：连接 → 读 DPI / 宽度 → 打印

### P8 待抓包

- [ ] `0x49`
- [ ] `0x4d`
- [ ] `0x78`（必须读回包，格式不明）
- [ ] `0x7e`（会重启）
- [ ] `0x83`（peripheralFlags）
- [ ] `0x84`（batteryCount / hw / sw flags）
- [ ] `0x88`（打印头温度 / 电池电压，两个子命令）
- [ ] `0x9e`（握手）
- [ ] `0x9f`（芯片 ID / 生产日期 / CRC / stack，四个子命令）

## 待确认 / 风险

- [x] RLE 的 `ManualPack` 修复已复测：字节级 `0x29` 正常，位级 `0x2c` / `0x3c` 正常
- [ ] 「尾部白色不编码、解码端补白」在 X 模式下已验证；「D 模式保留上一行」只在连续 D 时可能成立，X / D 混用已确认会断层
- [ ] `0x3a`（RLE4X）在 DP27P 上仍空白，待换机型或固件验证
- [ ] `cmd-impl-status.md` 的 `0x9e` resp 列现在是「无」，需要修正
- [ ] `0x70` / `0x72` 响应长度随固件变化，只从头部取固定字段，其余放 tail
- [ ] `0x77` 的 2 字节变长在 SDK 读取侧是反的，那是 SDK bug，统一按写入侧约定
- [ ] `0x77` 的 `status` 位语义：`0` 正常、`0x0400_0000` 等待数据、其余错误；尚未拿到干净回包验证
- [ ] `0x73` 两批抓包都不满足 `PrintLines + NullLines == WorkLines`；DP27P 新样本里 `PrintLines` 约是 `WorkLines` 的 2 倍，字段顺序 / 语义待复核
- [ ] `0x66` 在 DP27P USB 上多种变体均无响应；设置 `0x00` / `0x01` / `0x02` 也不改变 `0x42`，写命令假设未证实
- [ ] `0x7f` 在 DP27P 上固定回 16 字节全 `00`；激活状态、1/2 字节子索引、16 字节载荷都不改变回包，字段语义仍未知
- [ ] `0x80` 结束激活时设备回 `0x80` 而不是 `0x00`，`0x49` 的响应还没抓过
- [ ] `0x70` 请求的 `01` 参数（SDK `n1(true)` 分支）尚未确认用途，当前只发空载荷
- [ ] USB 报告的 `1e <长度>` 按旧 `packager` 实现为「本片命令区长度」，不是整段长度；切片的长度语义需要真机确认
- [ ] `ModelVersion` 判定依据：协议只写了 SDK `h=0/16/303`，没有从设备读取 `device_type` / `h` 的实现；DP27P 旧代码走 v1，DP305P 暂按 v2。实测 DP27P（`DP27P-Y4094C023`，`3.1.20230620`）在 v2 编码下能查询 `0x70`~`0x7d`，打印命令还没验证
- [ ] `0x88` 校验歧义：v2 的合法校验和恰好为 `0x88` 时会被当成「不校验」而跳过验证；与 SDK 行为一致，暂不处理
- [ ] 每完成一个阶段，同步更新 `cmd-impl-status.md` 的 resp 列；确认过的格式补到 `protocol.md`
- [x] 新增 `resp/` 后补 `README.md` 的代码结构说明

## 代码质量审核（2026-11-01）

> 只读审核结论：`cargo test --lib` 318 passed；`cargo fmt --check` 通过；
> `cargo clippy --all-targets` 当时有 `test_usb` 的 dead_code 和 11 个未使用依赖；
> `cargo doc --no-deps --lib` 1 个文档链接警告。审核期间未修改源码。
>
> P0 完成情况（2026-11-01）：`cargo test --lib` 326 passed、`cargo test --bins` 2 passed；
> `cargo clippy --all-targets` 已无 dead_code 警告，只剩未使用依赖。
>
> worker 托管（2026-11-01）：改用 `mytm` 后 `cargo test --all-targets` 331 lib + 2 bin passed。
>
> 通道迁移（2026-11-01）：`out_tx` / payload / ack 换成 `crossfire` 后仍是
> 332 lib + 2 bin passed；`clippy --all-targets` 无源码警告，`doc` 0 warning。

### P0 结构：frontend 与 test_usb

> 本节的 P0 与上面「规划：frontend / 设备线程 / transport」的未完成项重叠，
> 完成时两边一起勾选。当前两边已同步。

- [x] 把 `TestFrontend` / `RecvState` / `bg` / `drain_frames` / `wait_for_print_done`
  从 `src/bin/test_usb.rs` 搬到 `src/frontend/`
  - 拆分：`frontend/mod.rs`（`Device` / `DeviceConfig` / 收发 / 等待完成）、
    `frontend/worker.rs`（worker / 切帧 / `OutJob`）、
    `frontend/print.rs`（`print_bitmap_job`）
  - 用内存 transport 给发送、`recv_matching`、等待完成和打印任务补了 7 个单测
  - `src/bin/test_usb.rs` 只保留 `main`、`phase_*`、clap 参数和测试图
- [x] 把 `print_commands_to_reqs` 搬进库：`planner::print::into_reqs`
  - `PrintCommand` 现在由库内统一翻译成 `Box<dyn ReqTrait>`
- [x] `test_usb` 的阶段选择改用 clap：`--phase` / `--rle` / `--rows`，
  不再读取环境变量，12 个 dead_code 警告已消除
- [x] worker 改用 `mytm` 托管（不再裸用 `tokio::task::spawn_blocking`）
  - `TaskManager` 由调用方在最外层创建，通过 `Device::new(&tm, config)`
    传给 worker；退出前 `tm.close()`
  - `TaskManager::spawn_blocking_typed` 接管阻塞线程，panic 变成 `TaskState::Panicked`
  - `Device::ready()` 等任务进入 `Running`（worker 打开设备后 update 一次），
    删除 `ready` 的 `watch` 通道
  - 新增 `Device::shutdown(timeout)`：`cancel()` + 带超时的 `wait_terminal()`，
    阻塞线程真正返回后 USB 句柄才释放
  - `test_usb` 退出前（成功或失败）都调用 `shutdown`，然后 `tm.close()`
  - 新增 4 个单测：打开失败、shutdown 停线程、worker 卡住时 shutdown 超时、
    外层 `close()` 停掉 worker（后续补了 `shutdown` 幂等和 payload 通道关闭断言）
- [x] 第二步：`out_tx` / payload / ack 换成 `crossfire`
  - 写任务：`mpmc::bounded_async_blocking::<OutJob>(8)`，前台拿异步、可克隆的
    `MAsyncTx`（`Sync`，`Device` 仍满足 `Send + Sync`），worker 拿阻塞 `MRx`
  - payload：`spsc::unbounded_async::<Vec<u8>>()`，worker 用阻塞 `Tx` 发送，
    前台用 `AsyncRx` 接收
  - ack：`crossfire::oneshot`，worker 侧 `send` 不返回 `Result`（接收端消失时静默丢弃）
  - `ready` 已经不用通道，不受影响
  - 真机复测（DP27P）：`device-info` / `flow-control` /
    `--phase print --rle bits5x --rows 200` 均正常，`print completed` 后
    worker 在 100 ms 内取消退出、无 IO 错误

### P1 拆分长文件 / 命名

- [ ] `src/planner/print.rs`（含 `into_reqs`）拆成 `print/mod.rs`
  （`RleMode` / `PrintCommand` / `PrintPage` / `PrintError`）、
  `print/rle.rs`（`select_rle`）、`print/tests.rs`
- [ ] `src/rle.rs` 拆成 `rle/mod.rs`、`rle/bit.rs`、`rle/byte.rs`、`rle/tests.rs`
  - 补模块级 `//!`，链接 `rle.md`
  - `TryInto<Vec<Run<_>>>`（`src/rle.rs:31-87`）改成命名方法 `runs_bits()` / `runs_bytes()`
- [ ] 大文件的测试移到独立子模块：`bitmap.rs`（约 180 行测试）、
  `varint.rs`（约 165）、`settings.rs`（约 130）、`assembler.rs`
- [ ] `src/planner/usb.rs` 改名为 `transport/usb_frame.rs` 或
  `backend/usb_frame.rs`，避免和 `backend/usb.rs` 混淆
- [ ] 统一 RLE 命名：`RleMode::Bits4` / `RleKind::Bits4x` / `PrintCommand::Rle4x`
  保留一套命名，或在文档里补类型对照表

### P1 机械重复

- [ ] `EnvelopeKind` 的 20 处 `match mv` 收敛成
  `impl From<ModelVersion> for EnvelopeKind`
- [ ] 46 处 `to_bytes().context(EncodeSnafu { cmd_name })` 用宏或 helper 收敛
- [ ] RLE 的 7 处映射（`RleMode`、`PrintCommand`、`RleKind`、`select_rle`、
  `PrintLineRle::pack`、`req_template`、`planner::print::into_reqs`）用宏生成，
  新增变体只改一处
- [ ] 16 份 `fn hexdec` 合并到 `#[cfg(test)] test_util`
- [ ] `_tail: ()` 41 处：只在需要尾部 magic 时保留，其余删掉或改名
- [ ] `VarintMode::encode_into_safe` 复用 `encode_into`；当前无人使用
- [ ] `SpeedSetting` / `DarknessSetting` 的 `from_user` / `to_wire` / `from_wire`
  可抽公共宏

### P1 正确性 / 健壮性

- [ ] 响应队列按命令字节分发
  - `Device::recv_matching` 已搬进 `src/frontend/mod.rs`，但
    `frontend/print.rs` 的预算查询和 `wait_for_print_done` 仍用 `try_recv_one`
  - `0x7b` 或未知状态仍可能卡在队首，导致预算查询退化或等待完成报错
  - 方案：两处都改用 `recv_matching`，不匹配的帧丢弃并记录
- [ ] `wait_for_print_done` 跳过非 `0x70` 帧，不因 `0x7b` 推送退出
- [ ] D 模式在 `Feed` 之后强制只用 X：`src/planner/print.rs` 的
  `prev = bm.line(y - 1)` 可能取到已合并成 `Feed` 的空白行
  - 补「空白行后接 D 行」的测试
- [ ] 修正文档矛盾：`src/planner/print.rs:18-19` 说 D 保留普通 Line，
  `rle.md:21-24` 说 X / D 不能混用
  - `RleMode::Bits4/5/6` 改名 `*Auto`，或加「DP27P 不可用」警告
- [ ] `PrinterStatus` 支持未知值：`Unknown(u8)` 或
  `Status { raw, status: Option<_> }`
  - 避免 `0x0c+` 新错误码变成 `ParseRespError::InvalidValue` 并卡队列
- [ ] `FlowPlanner::on_buffer_size` 检查 `BufferSize::is_error()`；
  错误状态中止或退化，不继续按预算发送
- [ ] `PrintLine` 参数校验：`src/command/req/print_line.rs:62` 的 v0
  `data_len as u16`、`:91` 的 v1 `dots as u16` 会静默截断
  - 用 `try_from` 返回 `ParamOutOfRange`
- [ ] `FeedLine` 避免超大分配：`src/command/req/feed_line.rs:40-52` 在
  `lines = u32::MAX` 时会生成上千万段
- [ ] USB 接口 detach 后 reattach：`src/backend/usb.rs:289-300` 没有
  `attach_kernel_driver`
  - 用 `set_auto_detach_kernel_driver(true)` 或 Drop 清理；
    `Transport::reset`（`:186-190`）后重新 claim
- [ ] 日志降噪：`src/frontend/mod.rs` 的 `try_send_batch` / `send_cmd_area`
  在 debug 打印完整 9000 字节 hex，且两处重复
  - debug 只打 `bytes` 和摘要，完整 hex 放 trace
- [ ] 三处漏网的数组打印改 `simple_hex`：
  - `src/planner/assembler.rs:121` `{bytes:02X?}`
  - `src/planner/usb.rs:38` `{bytes:02X?}`
  - `src/command/resp/gap_length.rs:45` `{raw:02X?}`

### P2 死代码 / 依赖 / 文档

- [ ] 处理全注释的 `src/bin/dzprint.rs`（207 行）和
  `src/bin/dzprint_typst.rs`（762 行）
  - 移到 `examples/`、加 feature gate 或删除；同时清掉未使用依赖
    （当前是 `btleplug` / `bytebuffer` / `chrono` / `num-derive` / `strum` /
    `tar` / `tempfile` / `typst` / `typst-render` / `ureq`；`crossfire` 已用于
    frontend 的通道）
- [ ] `src/bin/dzcli.rs` 只有空 `main_fn`，先实现 `List` 或标记未完成
- [x] 删除 `_gen/`（生成脚本只解析不生成，避免继续误导）
- [ ] 删除 `src/lib.rs` 末尾的空 dummy test
- [x] 去掉 `#![feature(iter_map_windows)]`：
  `src/rle.rs` 改为迭代器求游程边界，不再依赖 nightly feature
- [ ] 删除或使用 `Bitmap::bits_mut`（`src/bitmap.rs:96`）和
  `VarintMode::encode_into_safe`（`src/command/varint.rs:207`）
- [ ] 补模块级文档：`rle.rs`、`settings.rs`、`command/mod.rs`、`command/envelope.rs`
- [x] 修 `src/planner/mod.rs` 的 `[print]` 文档链接（模块/宏重名）
- [ ] `TODO.md` 的「进行中」改成「已完成记录」，另开「下一步」
- [ ] `README.md` 的 `bitmap.rs` 描述改成同时包含 `Bitmap` 和 `BitmapLine`
- [ ] `protocol.md` / `flow-control.md` 考虑拆分「规范」和「实验记录」

### 验收

- [ ] `cargo test --lib` 不低于当前 332 passed
- [ ] `cargo clippy --all-targets` 无 dead_code / 未使用依赖警告
- [ ] `cargo fmt -- --check` 通过
- [ ] `cargo doc --no-deps --lib` 无警告

## 文档同步规则

新增或修改命令模板时：

- [ ] 同步 `cmd-impl-status.md` 的 req / resp 列
- [ ] 同步 `protocol.md` 的字节格式
- [ ] 源码链接保持可点击

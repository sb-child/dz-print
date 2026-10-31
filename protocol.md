# 底层协议

## [变长编码](src/command/varint.rs)

## [RLE 编码](rle.md)

## 单位

- 1个物理毫米(mm) = `25.4 / DPI`
- 1个点(dot) = 位图的一个像素点 = 1打印行
- 不同型号打印机的 DPI 可能不同。

## 位图

`位图数据` 代表：

- 一行字节，从左到右的每一个比特代表一个点(dot)。
- 该点为比特1，代表打印(黑色)。该点为比特0，代表空白(白色不打印)。

## 字符串

字符串的编码可能是 GBK/GB2312。字符串有后缀 `0x00`。

## 机型区别对待

为了方便区分，下面用 `vx` 代表这 3 个版本之一。

- `v2` 新机型。SDK: `h=0`
- `v1` 过渡机型。SDK: `h=16`
- `v0` 最老机型。SDK: `h=303`

## 传输协议 `双向`

- 包头 `USB`
  - `命令总长度` < 192
    ```
    1e <1 byte 命令总长度> [命令...]
    ```
  - `命令总长度` >= 192
    ```
    1e <2 bytes 命令总长度> [命令...]
    ```

> **这个 `0x1e` 其实是 HID Report ID**(DP305P 实测)。DP305P 的 HID 接口报告
> 描述符为 `Report ID 0x1e / Report Size 8 / Report Count 63`, 也就是每份报告
> 1 字节报告号 + 63 字节数据; 所以线上帧天然就是 `1e <长度> <命令...>`。
> 由此可得两点:
>
> - 命令区**不需要**补齐到固定长度 (如 62 字节), 只要整份 64 字节报告补齐即可;
>   实测命令区不补也是正常的。
> - 走非 HID 接口时, `0x1e` 只是一个普通数据字节, 语义不变。
>
> HID 报告里留给命令区的最多 62 字节 (`64 - 1 字节报告号 - 1 字节长度字段`)。
> 超过 62 字节的命令区要按 62 字节切片, 每片各自带 `1e <片段长度>`, 报告尾部补
> `0x00` 到 64 字节; 接收方按长度字段裁掉补齐的零。

- 包头 `BLE`

  ```
  [命令...]
  ```

  > (已验证, DP305P) BLE 是**串口透传**，没有额外包头。USB 上的 `0x1e` 其实是
  > HID 的 Report ID，BLE/串口没有报告号，命令区原样写入即可；同一组参数下
  > BLE 与 USB 的命令区逐字节相同。GATT、响应和流控见文末
  > 「[蓝牙 (BLE) 传输](#蓝牙-ble-传输)」。

- 命令 `USB && BLE`

  命令按照顺序依次打包/解析
  - `数据长度` < 192
    ```
    <1 byte 命令组> <1 byte 命令类型> <1 byte 数据长度> [数据...] [1 byte 校验和 初始=0]
    ```
  - `数据长度` >= 192
    ```
    <1 byte 命令组> <1 byte 命令类型> <2 bytes 数据长度> [数据...] [1 byte 校验和 初始=0]
    ```
  - 校验和
    - 如果未启用校验, 则为 `0x88`
    - 否则使用[这个](src/command/mod.rs)函数计算 `命令[1..命令长度]` 的校验和

---

[查看简洁版进度](./cmd-impl-status.md)

## 命令列表 - 裸命令

- `0x0c` 结束页命令(结束一页)。`0c`。无参数。仅`v1`机型。`rust:EndPageV1Tl` [代码](src/command/req_template/end_page.rs)

## 命令列表 - 命令组 `0x1a` (仅`v0`机型)

- `0x3_`
  - `0x37` 水平偏移 `rust:PageOffsetV0Tl` [代码](src/command/req_template/page_offset.rs)
    - `1a 37 <n>`
      - `n`: 水平偏移*毫米* + 150。1字节。`n = (偏移px / (DPI/25.4)) + 150`
  - `0x38` 页开始参数。`rust:StartPageSeq1V0Tl` [代码](src/command/req_template/start_page_seqs_v0.rs)
    - `1a 38 01` (参数固定 `0x01`)
  - `0x39` 页开始参数。`rust:StartPageSeq2V0Tl` [代码](src/command/req_template/start_page_seqs_v0.rs)
    - `1a 39 01` (参数固定 `0x01`)
  - `0x3a` 页开始参数。`rust:StartPageSeq3V0Tl` [代码](src/command/req_template/start_page_seqs_v0.rs)
    - `1a 3a 02` (参数固定 `0x02`)

## 命令列表 - 命令组 `0x1b`

- `0x4_`
  - `0x40` 开始页命令 `rust:StartPageV0Tl` [代码](src/command/req_template/start_page.rs)
    - `v2`机型: 见 `1f 20`。
    - `v1`机型: 没有这种命令(不需要开始页)。
    - `v0`机型: `1b 40`。无参数。
  - `0x4a` 走纸 `rust:FeedLineV1V0Tl` [代码](src/command/req_template/feed_line.rs)
    - `v2`机型: 如果 `BLE && n <= 255` 才会使用 `1b 4a <n>`。否则见 `1f 22`。
    - `v1,v0`机型: `1b 4a <n>`: 走纸 `n` 行。取值 `1 ~ 255`。

## 命令列表 - 命令组 `0x1d` (仅`v0`机型)

- `0x5_`
  - `0x56` 结束页命令(结束一页): `1d 56 42 00`。无参数。走纸到标签间隙。`rust:EndPageV0Tl` [代码](src/command/req_template/end_page.rs)
- `0x7_`
  - `0x76` 打印一行: `1d 76 30 00 <Len:定长> 01 00 <Data>` | `rust:PrintLineV0Tl` [代码](src/command/req_template/print_line.rs)
    - `Len`: 位图数据字节数，2字节小端(u16LE)。
    - `Data`: 位图数据。

## 命令列表 - 命令组 `0x1f`

- `0x2_` 打印命令 (特殊命令)
  - `0x20` 开始页命令: `rust:StartPageV2Tl` [代码](src/command/req_template/start_page.rs)
    - `v2`机型: `1f 20 <PAGE_KEY:定长> 00 00 00 00 <PRINT_SEPARATE_LINE> 00`
      - `PAGE_KEY`: 本页的编号，2字节大端(u16BE)。范围 `1~65534`，溢出重置到 `1`。
      - `PRINT_SEPARATE_LINE`: 是否打印分隔线，1字节。取值 `0 | 1`。
    - `v1`机型: 没有这种命令，不需要开始页。
    - `v0`机型: 见 `1b 40`。
  - `0x21` 打印一行: `rust:PrintLineV2Tl` [代码](src/command/req_template/print_line.rs)
    - `v2`机型: `1f 21 <Repeats:变长> <Skips:变长> <Data>`
      - `Repeats`: 重复次数 - 1。0 代表这一行只打印一次。取值 `0~16383`。
      - `Skips`: 向右偏移的字节数。取值 `0~191`。
      - `Data`: 位图数据。
    - `v1`机型: 见 `1f 2b`。
    - `v0`机型: 见 `1d 76`。
  - `0x22` 走纸 `rust:FeedLineV2Tl` [代码](src/command/req_template/feed_line.rs)
    - `v2`机型: `1f 22 <n:变长>`
      - `n`: 走纸行数 - 1。取值 `0~16383`。
      - 如果 `BLE && (n+1) <= 255`，应使用 `1b 4a <n2>`，此处的 `n2` 取值 `1~255`。
    - `v1,v0`机型: 见 `1b 4a`。
  - `0x23` (X)
  - `0x24` (X)
  - `0x25` 加热控制 `rust:HeatControlV2V1V0Tl` [代码](src/command/req_template/heat_control.rs)
    - `v0,v1,v2`机型: `1f 25 <StartDots:VarFixed2> <EndDots:VarFixed2>`
  - `0x26` 设置本次打印总行数(包括空白行)
    - `v2`机型: `1f 26 <Lines:变长>` `rust:TotalLinesV2Tl` [代码](src/command/req_template/total_lines.rs)
    - `v1`机型: `1f 26 <Lines:C0Be16>` `rust:TotalLinesV1Tl` [代码](src/command/req_template/total_lines.rs)
    - `v0`机型: 没有这种命令。
  - `0x27` 设置标签宽度 `rust:PageWidthV2V1Tl` [代码](src/command/req_template/page_width.rs)
    - `v2,v1`机型: `1f 27 <Width>`: `Width` = (宽(dot)+7)/8, 1字节。
    - `v0`机型: 见 `1a 37`。
  - `0x28` 结束页命令(结束一页) `rust:EndPageV2Tl` [代码](src/command/req_template/end_page.rs)
    - `v2`机型: `1f 28`，无参数。
    - `v1`机型: 见 `0c`。
    - `v0`机型: 见 `1d 56`。
  - `0x29` 打印RLE*字节级*压缩的一行 `rust:PrintLineRleByteV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 29 <RleByteLen:变长> <RleData>`。编码失败回退 `1f 2b`。
      - `RleByteLen`: RLE数据字节数。
      - `RleData`: RLE数据。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x2a` 打印一行 `rust:PrintLineV1Tl` [代码](src/command/req_template/print_line.rs)
    - `v2`机型: 见 `1f 21`。
    - `v1`机型: `1f 2a <Dots:定长> <Data>`
      - `Dots`: 位图数据的点数(位数)，2字节小端(u16LE)。
      - `Data`: 位图数据。
    - `v0`机型: 见 `1d 76`。
  - `0x2b` 打印一行 `rust:PrintLineV2V1Tl` [代码](src/command/req_template/print_line.rs)
    - `v2,v1`机型: `1f 2b <Skips:变长> <DataLen:变长> <Data>`
      - `Skips`: 向右偏移的字节数。取值 `0~191`。
      - `DataLen`: 位图数据长度。取值 `0~191`。
      - `Data`: 位图数据。
    - `v0`机型: 见 `1d 76`。
  - `0x2c` 打印RLE*位级*压缩的一行(RLE5X) `rust:PrintLineRle5xV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 2c <符号数:变长> <5位位流>`。编码失败回退 `1f 2b`。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x2d` 打印RLE*位级*压缩的一行(RLE5D) `rust:PrintLineRle5dV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 2d <符号数:变长> <5位位流>`。编码失败回退 `1f 2b`。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x2e` 重复上一行 `rust:RepeatLineV2V1Tl` [代码](src/command/req_template/repeat_line.rs)
    - `v2`机型: `1f 2e <n:变长>`: `n` 取值 `0~16383`，重复打印 `n + 1` 行。
    - `v1`机型: `1f 2e <n:变长>`: `n` 取值 `0~191`，重复打印 `n + 1` 行。
    - `v0`机型: 没有这种命令。应重发 `1d 76` 命令实现。
  - `0x2f` (X)

- `0x3_` 打印命令 (特殊命令)
  - `0x30` (X)
  - `0x31` (X)
  - `0x32` (X)
  - `0x33` (X)
  - `0x34` (X)
  - `0x35` (X)
  - `0x36` (X)
  - `0x37` (X)
  - `0x38` (X)
  - `0x39` (X)
  - `0x3a` 打印RLE*位级*压缩的一行(RLE4X) `rust:PrintLineRle4xV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 3a <符号数:变长> <4位位流>`。编码失败回退 `1f 2b`。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x3b` 打印RLE*位级*压缩的一行(RLE4D) `rust:PrintLineRle4dV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 3b <符号数:变长> <4位位流>`。编码失败回退 `1f 2b`。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x3c` 打印RLE*位级*压缩的一行(RLE6X) `rust:PrintLineRle6xV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 3c <符号数:变长> <6位位流>`。编码失败回退 `1f 2b`。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x3d` 打印RLE*位级*压缩的一行(RLE6D) `rust:PrintLineRle6dV2Tl` [代码](src/command/req_template/print_line_rle.rs)
    - `v2`机型: `1f 3d <符号数:变长> <6位位流>`。编码失败回退 `1f 2b`。
    - `v1`机型: 没有这种命令。见 `1f 2b`。
    - `v0`机型: 没有这种命令。见 `1d 76`。
  - `0x3e` (X)
  - `0x3f` (X)

- `0x4_` 打印设置 (可读取写入)
  - `0x40` (X)
  - `0x41` (X)
  - `0x42` 打印纸类型 [见这里](#打印纸类型) `rust:GapTypeV0V1V2Tl` [代码](src/command/req_template/gap_type.rs)
  - `0x43` 打印浓度 [见这里](#打印浓度) `rust:DarknessV2V1V0Tl` [代码](src/command/req_template/darkness.rs)
  - `0x44` 打印速度 [见这里](#打印速度) `rust:SpeedV2V1V0Tl` [代码](src/command/req_template/speed.rs)
  - `0x45` 打印纸间隔 [见这里](#打印纸间隔) `rust:GapLengthV2Tl` [代码](src/command/req_template/gap_length.rs) 非连续纸才生效 `v2`机型支持，`v1,v0`机型也许支持。
  - `0x46` (X)
  - `0x47` 电机模式
  - `0x48` 自动关机时间(分钟)
    - 调用/返回: `AutoPowerOffMins` 1字节或2字节BE。0=不自动关机。
  - `0x49` # TODO
  - `0x4a` (X)
  - `0x4b` (X)
  - `0x4c` (X)
  - `0x4d` # TODO
  - `0x4e` (X)
  - `0x4f` (X)

- `0x5_` 设备信息 (可读取)
  - `0x50` (X)
  - `0x51` (X)
  - `0x52` 支持的纸张类型
  - `0x53` (X)
  - `0x54` (X)
  - `0x55` (X)
  - `0x56` (X)
  - `0x57` 支持的电机模式
  - `0x58` (X)
  - `0x59` 支持的语言
  - `0x5a` (X)
  - `0x5b` (X)
  - `0x5c` (X)
  - `0x5d` (X)
  - `0x5e` (X)
  - `0x5f` (X)

- `0x6_` 设备信息 (可读取)
  - `0x60` (X)
  - `0x61` (X)
  - `0x62` (X)
  - `0x63` (X)
  - `0x64` (X)
  - `0x65` (X)
  - `0x66` 获取打印纸类型
    - 返回: `<PaperType>`。1字节:
      - `0x00` Ticket 小票纸(连续纸)
      - `0x01` Adhesive 不干胶(定位孔)
      - `0x02` CardPaper 卡纸(间隙纸)
      - `0x03` Transparent 透明贴(黑标纸)
  - `0x67` (X)
  - `0x68` (X)
  - `0x69` (X)
  - `0x6a` (X)
  - `0x6b` (X)
  - `0x6c` (X)
  - `0x6d` (X)
  - `0x6e` (X)
  - `0x6f` (X)

- `0x7_` 设备信息 (可读取)
  - `0x70` 获取打印机状态 `rust:StatusV2V1V0Tl` [代码](src/command/req_template/status.rs)
    - [打印状态](print-status.md)
  - `0x71` 获取设备DPI
    - 返回: `<Dpi:u16BE>`: 打印头DPI(比如`300`)
  - `0x72` 获取打印宽度和出纸宽度
    - 返回: `<PrintWidth:u16BE> <PaperWidth:u16BE> [printerLocateArea] 00 00 00 00`
      - `PrintWidth`: 打印头点数(比如`576`)
      - `PaperWidth`: 支持最大纸宽(0.1mm)(比如`570`=57.0mm)
      - `printerLocateArea`: 1字节。定位区域。可选参数。
  - `0x73` 获取打印统计信息 # TODO 可能有理解偏差
    - 返回: `<WorkLines:u32BE> <PrintLines:u32BE> <NullLines:u32BE> <PrintPages:u32BE>`
      - `WorkLines`: 电机一共走过的行数。
      - `PrintLines`: 其中打印过的行数。
      - `NullLines`: 其中走过的空行数。
      - `PrintPages`: 打印过的总页数。
  - `0x74` (X)
  - `0x75` 制造商
    - 返回: `<Manufacturer:string>`
      - `Manufacturer`: 制造商名称。字符串。
  - `0x76` (X)
  - `0x77` 缓冲容量查询（流控核心），见 [flow-control.md](flow-control.md)
  - `0x78` 时间同步 (可读取写入) # TODO 到底怎么用
    - 无参数调用: 查询某设备信息？
    - 调用: `1f 78 [TimeZone:2bBE] [Year:u16BE] [Month] [Day] [Week] [Hour] [Minute] [Second]` 设置时间。# TODO: 为什么BLE连接要加100分钟?
      - `TimeZone`: 不明字段。BLE连接发送`6`，USB连接发送`16`。# TODO
      - `Year` `Month` `Day`: 年，月，日。
      - `Week`: 星期(周日=7)。
      - `Hour` `Minute` `Second`: 时，分，秒。
    - 返回 # TODO 不明确，但必须接收返回值。
  - `0x79` 设备名
    - 返回: `<DeviceName:string>`
      - `DeviceName`: 设备名，格式 `型号-序列号`
  - `0x7a` 硬件版本
    - 返回: `0x26, 0x01, 0x04, 0x01, 0x04` = 硬件版本 2.6 (`"0x26"` -> `2.6`)
  - `0x7b` 缓冲容量查询 2，见 [flow-control.md](flow-control.md)
  - `0x7c` 软件版本 # TODO
  - `0x7d` 蓝牙mac地址
    - 返回: `10 <BtMacAddr:6bytes> 14 <BtMacAddr:6bytes>`
      - `BtMacAddr`: 蓝牙mac地址，示例 `60 6e 41 37 c4 37`=`60:6e:41:37:c4:37`。不知道为什么会重复两次，可能是经典蓝牙地址 + BLE 地址标识。
  - `0x7e` # TODO 打印机会重启
  - `0x7f` # TODO
- `0x8_`
  - `0x80` 激活参数 # TODO
  - `0x81`
  - `0x82`
  - `0x83`
  - `0x84` 握手 # TODO
  - `0x85`
  - `0x86`
  - `0x87`
  - `0x88` 设备信息查询 # TODO
  - `0x89`
  - `0x8a`
  - `0x8b`
  - `0x8c`
  - `0x8d`
  - `0x8e`
  - `0x8f`
- `0x9_`
  - `0x90`
  - `0x91`
  - `0x92`
  - `0x93`
  - `0x94`
  - `0x95`
  - `0x96`
  - `0x97`
  - `0x98`
  - `0x99`
  - `0x9a`
  - `0x9b`
  - `0x9c`
  - `0x9d`
  - `0x9e` 握手 # TODO
  - `0x9f`

---

以下是比较旧的文档

## TODO

- (已验证) `0x52` SupportedGapTypes 支持的纸张类型 `02, 03, 04, 00`
  - 依次为 Adhesive / CardPaper / Transparent / Ticket, 与 `0x42` 的取值一致

- (已验证) `0x57` SupportedMotorModes 支持的电机模式 `0c`

- `0x59` # TODO SupportedLanguages 支持的语言 `61, 01`

- `0x5b` # TODO `00, 00`

- 连接方式 也许是打印机类型 (# FIXME 任何参数都返回 `0x02 0x43`) `主机->设备` `0x78`
  - 参数 `BLE`
    - `[0]` `0x06` `(i8)`
  - 参数 `USB`
    - `[0]` `0x10` `(i8)`

- (已验证) `0x70` 打印机状态
  - `[0]` 状态码, 见 [打印状态](print-status.md); `0x00` = 正常
  - `[2:1]` PageKey, 大端 16 位
  - `[3..]` 其余字段未确认
  - **载荷长度随固件变化**(DP27P 8 字节, DP305P 28 字节), 只应按 `[0]` 判错,
    不要按固定长度解析
  - DP27P: `00, 01, 02, 00, 00, 35, 01, 10`
  - DP305P: `00, 01, 02, 00, 00, 33, 01, 24, 00, ..., 0C, DC` (28 字节)

- `0x73` # TODO 打印统计 `00, 01, b6, 4b, 00, 01, 76, ce, 00, 00, 71, 58, 00, 00, 00, de`
  - workLines
  - printLines
  - nullLines
  - printPages

- `0x77` # TODO `32, 00, 00, 00, 00, 88, 00` `32, 04, 00, 00, 20, 88, 00`
  - 0x32 = 0b00110010

- `0x7a` # TODO 硬件版本 `0x26, 0x01, 0x04, 0x01, 0x04`

- `0x7d` # TODO 蓝牙mac地址 `10, 60, 6e, 41, 37, c4, 37, 14, 60, 6e, 41, 37, c4, 37` `60:6e:41:37:c4:37`

- (已验证) `0x71` DPI
  - `[1:0]` DPI, 大端 16 位
  - DP27P / DP305P 均为 `01, 2c` = 300

- (已验证) `0x72` 打印宽度 / 纸宽
  - `[1:0]` 打印点数, 大端 16 位
    - DP27P `02, 40` = 576 点 (48.8mm @300dpi)
    - DP305P `03, C0` = 960 点 (81.28mm @300dpi)
  - `[3:2]` 纸宽, 单位 0.1mm, 大端 16 位
    - DP27P `02, 3a` = 570 = 57.0mm
    - DP305P `03, 84` = 900 = 90.0mm
  - 其后字节含义不明, 长度也随固件变化 (DP27P 9 字节, DP305P 11 字节),
    只取前 4 字节即可

- `0x7b` # TODO `32`

- `0x7e` # TODO 打印机会重启

- `0x7f` # TODO `00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00`

### 电池数量, hardwareFlags, softwareFlags

- (# FIXME) `主机->设备` `0x84`
  - 无参数

```java
byte[] byteArray = dataPackage.toByteArray(); // 0x84
if (byteArray.length >= 25) {
  this.printer_param_2.batteryCount = byteArray[23] & 255;
} else {
  this.printer_param_2.batteryCount = 2;
}
this.printer_param_2.hardwareFlags = dataPackageReader.readInt();
if (dataPackageReader.getRemainingLength() >= 4) {
  this.printer_param_2.softwareFlags = dataPackageReader.readInt();
} else {
  this.printer_param_2.softwareFlags = 1 | (this.printer_param_2.hardwareFlags & 16);
}
```

### peripheralFlags

- (# FIXME) 读取 peripheralFlags `主机->设备` `0x83`
  - 无参数
- (# FIXME) peripheralFlags `设备->主机` `0x83`
  - 如果数据长度 > 1 且 `[0]` = `0x01`:
    - `[1]` peripheralFlags
  - 否则
    - `[0]` peripheralFlags

### 打印头温度/电池电压和充电状态

- (# FIXME) 读取打印头温度/电池电压和充电状态 `主机->设备` `0x88`
  - 参数 读取打印头温度
    - `[0]` `0x01`
  - 参数 读取电池电压和充电状态
    - `[0]` `0x02`
- (# FIXME) 打印头温度 `设备->主机` `0x88`
  - `[0]` `0x01`
  - `[2:1]` `((short) (((short) (((short) (0 | (readBytes[1] & 255))) << 8)) | (readBytes[2] & 255))) * 0.1d`
- (# FIXME) 电池电压和充电状态 `设备->主机` `0x88`
  - `[0]` `0x02`
  - `[1..]`
    ```java
    if (readBytes.length > 10) {
      this.printer_param_2.chargeStatus = readBytes[10] & 255;
    }
    if (readBytes.length > 8) {
      this.printer_param_2.batteryVoltage = (((readBytes[7] & 255) << 8) + (readBytes[8] & 255)) * 0.01d;
    }
    ```

### `0x9e` 未知

- (# FIXME) `主机->设备` `0x9e`

### 芯片ID/生产日期/升级CRC/stackHead, stackTail, heapHead, heapTail, maxStack, heapUnused, heapMinUnused

- (# FIXME) `主机->设备` `0x9f`
  - 参数 读取芯片ID
    - `[0]` `0x0a`
  - 参数 读取生产日期
    - `[0]` `0x11`
  - 参数 读取升级CRC
    - `[0]` `0x29`
  - 参数 读取 stackHead, stackTail, heapHead, heapTail, maxStack, heapUnused, heapMinUnused
    - `[0]` `0x61` `需要进一步确定`

### 激活 `> 0x80` 命令

- (# FIXME) `主机->设备` `0x80`
  - 发送 `> 0x80` 的命令前, 需要发送这个命令
  - 参数
    - `[0]` `0x7f`
- (# FIXME) `设备->主机` `0x80`
  - `[0]` `0x7f`

### 设备信息

#### 设备名

- (已验证) 读取设备名 `主机->设备` `0x79`
  - 无参数
- (已验证) 设备名 `设备->主机` `0x79`
  - `[..]` 以 `0x00` 结尾的设备名 `型号-序列号` `(str)`

#### 软件版本

- (已验证) 读取软件版本 `主机->设备` `0x7c`
  - 无参数
- (已验证) 软件版本 `设备->主机` `0x7c`
  - `[..]` 以 `0x00` 结尾的软件版本 `x.y yyyymmdd`

#### 制造商

- (已验证) 读取制造商 `主机->设备` `0x75`
  - 无参数
- (已验证) 制造商 `设备->主机` `0x75`
  - `[..]` 以 `0x00` 结尾的制造商名 `(str)`
    - `gbk` / `gb2312` 编码

#### 其他

- 读取信息 `主机->设备` `0x75`
  - (已验证) 无参数 制造商
  - 参数 系列 # FIXME
    - `[0]` `0x53`
  - 参数 内部型号 # FIXME
    - `[0]` `0x44`
- (已验证) 制造商 `设备->主机` `0x75`
  - `[..]` 以 `0x00` 结尾的制造商名 `(str)`
    - `gbk` / `gb2312` 编码
- 系列 `设备->主机` `0x75` # FIXME
  - `[..]` 以 `0x00` 结尾的系列名 `(str)`
- 内部型号 `设备->主机` `0x75` # FIXME
  - `[..]` 以 `0x00` 结尾的内部型号名 `(str)`

### 打印设置

#### 打印浓度

- (已验证) 设置打印浓度 `主机->设备` `0x43`
  - 参数
    - `[0]` 要设置的浓度值 `(i8)`
      - `0x00` 最淡 1
      - ...
      - `0x03` 较淡 4
      - ...
      - `0x05` 正常 6
      - ...
      - `0x09` 较浓 10
      - ...
      - `0x0e` 最浓 15
- (已验证) 读取打印浓度 `主机->设备` `0x43`
  - 无参数
- (已验证) 打印浓度 `设备->主机` `0x43`
  - **载荷长度随固件变化**(DP305P 为 4 字节 `05, 0f, 00, 5f`), 只取 `[0]`
  - `[0]` 打印浓度
    - `0x00` 最淡 1
    - ...
    - `0x03` 较淡 4
    - ...
    - `0x05` 正常 6
    - ...
    - `0x09` 较浓 10
    - ...
    - `0x0e` 最浓 15

#### 打印速度

- (已验证) 设置打印速度 `主机->设备` `0x44`
  - 参数
    - `[0]` 要设置的打印速度值 `(i8)`
      - `0x00` 最慢 1
      - `0x01` 较慢 2
      - `0x02` 正常 3
      - `0x03` 较快 4
      - `0x04` 最快 5
- (已验证) 读取打印速度 `主机->设备` `0x44`
  - 无参数
- (已验证) 打印速度 `设备->主机` `0x44`
  - **载荷长度随固件变化**(DP305P 为 12 字节), 只取 `[0]`
  - `[0]` 打印速度
    - `0x00` 最慢 1
    - `0x01` 较慢 2
    - `0x02` 正常 3
    - `0x03` 较快 4
    - `0x04` 最快 5

#### 打印纸类型

- (已验证) 设置打印纸类型 `主机->设备` `0x42`
  - 参数
    - `[0]` 要设置的打印纸类型 `(i8)`
      - `0x00` Ticket 小票纸
      - `0x02` Adhesive 不干胶
      - `0x03` CardPaper 卡纸
      - `0x04` Transparent 透明贴
- (已验证) 读取打印纸类型 `主机->设备` `0x42`
  - 无参数
- (已验证) 打印纸类型 `设备->主机` `0x42`
  - `[0]`
    - `0x00` Ticket 小票纸
    - `0x02` Adhesive 不干胶
    - `0x03` CardPaper 卡纸
    - `0x04` Transparent 透明贴

#### 打印纸间隔

- (已验证) 设置打印纸间隔 `主机->设备` `0x45`
  - 参数
    - `[0]` / `[1:0]` / `[2:0]` 要设置的打印纸间隔(0.01 mm)
      - 最小值 50 (0.5 mm)
- (已验证) 读取打印纸间隔 `主机->设备` `0x45`
  - 无参数
- (已验证) 打印纸间隔 `设备->主机` `0x45`
  - `[0]` / `[1:0]` / `[2:0]` 打印纸间隔(0.01 mm)
  - 按数据长度选解码方式: 1 字节直取, 2 字节 `0xC0|高位 低位`, 3 字节 `0xC0|高位 中位 低位`

#### 打印标签宽度

- 设置打印标签宽度 `主机->设备` `0x27`
  - 参数
    - `[0]` 要设置的打印标签宽度 (打印点数+7)/8
      > 设置打印标签的宽度, 取值为 (打印点数+7)/8。因为打印机打印分辨率为 8 个
      > 点每毫米, 因此取值其实为打印标签的宽度毫米数。
      > DT20 和 DP20 系列打印机的有效打印最大宽度均为 48 毫米。

#### 其他

- 电机模式 `主机->设备` `0x47`
  - 参数
    - `[0]` 未知 `(i8)` # TODO `00`

- `0x48` # TODO 自动关机时间(分钟) `00`
- `0x49` # TODO `61, 61, 10`
- `0x4d` # TODO `01, 02, 04, 00`

## 打印命令

- `0x1b 0x40` 初始化打印参数

  > 初始化打印参数到默认值, 开始打印一张标签的第一个命令。

- `0x1b 0x4a n` 走 N(0~255) 个空行

- `0x1f 0x2a nL nH [...]` 打印 N 点的一行, `[...]`为需要打印的数据, 其字节数为(n+7)/ 8。nL nH 是用两个

  > 字节表示的点数, 低字节在前, 高字节在后。
  > 打印数据按照从左到右的顺序排列, 一个比特位表示一个打印点, 比特位 1 表示需
  > 要打印的点, 0 表示不需要打印的点, 靠左的点在一个字节的较高位。
  > 如果指定的点数超过了标签的宽度, 则超出的打印内容将被忽略；如果指定的点数
  > 不足标签的宽度, 则默认缺少的数据为 0, 即为不需要打印的内容。

- `0x1f 0x2b m n [...]` 打印有前导空白的一行

  > 打印有前导空白的一行, m 为前导零的字节数(0 ~ 191), n 为打印数据的字节数
  > (0 ~ 191), `[...]`为需要打印的数据, 其字节数由 n 指定。
  > 打印数据按照从左到右的顺序排列, 一个比特位表示一个打印点, 比特位 1 表示需
  > 要打印的点, 0 表示不需要打印的点, 靠左的点在一个字节的较高位。
  > 如果指定的前导零点数(m _ 8) 加打印数据点数 (n _ 8)超过了标签的宽度, 则超
  > 出的打印内容将被忽略；如果指定的点数不足标签的宽度, 则默认缺少的数据为 0, 即
  > 为不需要打印的内容。
  > 本打印命令为 1F 2A 打印命令的数据压缩型命令, 对于一行打印数据有较多前导空
  > 白(前导零, 即无需打印内容)的情况, 使用本命令能有效地压缩打印数据量。

- `0x1f 0x2e n` 重复行

  > 打印和上一行一样内容的重复行。N 为重复行数-1, 比方说重复行数为 2, 则 N 的
  > 取值为 1; 重复行数为 192, 则 N 的取值为 191, 类推。
  > 注意: N 的取值要小于 192, 如果重复行数大于 192, 则需要拆分成多条打印重复
  > 行命令。比方说如果实际重复行数为 256, 则需要拆分成下面两条打印重复行命令: 1F
  > 2E BF 1F 2E 3F

- `0x0c` 定位到下一纸张边界
  > 定位到下一张标签的边界, 结束一张标签的打印。

## 机型 / 接口差异

### 复合设备: 两个接口只有一个走本协议

DP305P 是复合设备, 同一台机器上挂了两个接口:

| 接口 | 类                 | 端点                      | 本协议     |
| ---- | ------------------ | ------------------------- | ---------- |
| 0    | Printer (07/01/02) | Bulk `0x01` / `0x81`      | **不响应** |
| 1    | HID (03/00/00)     | Interrupt `0x02` / `0x82` | 正常       |

对接口 0 依次发了 12 条查询 (0x79 / 0x7c / 0x75 / 0x71 / 0x72 / 0x70 / 0x42 /
0x43 / 0x44 / 0x45 / 0x52 / 0x57), 两种补齐方式都试过, **零响应**;
同样的命令走接口 1 全部正确返回。

> 所以实现里选端点时应**优先同一接口的 Interrupt 端点对**, 不要只看"是不是
> Interrupt", 也不要指望 Bulk 端点能通。

### 打印中途查状态会在纸上留下一条白线

`0x70` 是需要设备回包的请求-响应命令。如果在**逐行发送打印数据的过程中**
插入状态查询并等待回包, 数据流会中断; 热敏头一缺数据就会在纸上留下一条
明显的白带 (DP305P 实测稳定复现)。

建议: 只在一张标签开始前和结束后查状态; 确实需要中途检测的话, 要保证
发送端有足够缓冲, 或者只在断点处查询并接受这个风险。

### 机型参数不要写死, 连上问设备

同一套协议在不同机型上只是参数不同:

|                   | DP27P           | DP305P           |
| ----------------- | --------------- | ---------------- |
| DPI (`0x71`)      | 300             | 300              |
| 打印头 (`0x72`)   | 576 点 (48.8mm) | 960 点 (81.28mm) |
| 最大纸宽 (`0x72`) | 57.0mm          | 90.0mm           |
| USB               | `3533:5c15`     | `3533:5c32`      |

所以拿到设备后先用 `0x71` / `0x72` 读真实 DPI 与打印点数, 再决定渲染尺寸,
比维护一张写死的型号表更可靠 —— 新机型不用改代码就能支持。

## 蓝牙 (BLE) 传输

实测机型: DeTong DP305P（BLE 模块 **品栋 PingDong PDBT33F**，模块固件 5.20）。
协议层结论: **BLE = 命令区去掉 USB 的 `0x1e` 外层，走串口透传**。
同一组参数下 BLE 与 USB 生成的命令区逐字节相同，`Assembler::encode()` 的输出
可以直接写 `ff02`。

### GATT 布局

```text
svc 0000ff00-0000-1000-8000-00805f9b34fb
  ff02  write / write-without-response   ← 主机→打印机（命令区，无包头）
  ff01  notify                           ← 打印机→主机（响应）
  ff03  notify                           ← 事件 / 流控信用
svc 0000ff10-…
  ff11  notify + write-without-response  ← 用途未知（疑似 OTA）
  ff12  notify + write-without-response  ← 同上
svc 0000180a-…  Device Information
  2a29 Manufacturer = "PingDong"
  2a24 Model        = "PDBT33F-DF4"
  2a25 Serial       = "28D41E3E99D5"      ← 与 `0x7d` 读出的 MAC 对应
  2a26 Firmware     = "5.20"
```

广播名与 USB 设备名同构：`型号-序列号`（如 `DP305P-Y60870279`）。
序列号每台不同，macOS 上 BLE 地址还是随机 UUID，所以只能按型号片段匹配。

### 响应帧

一次 `ff01` 通知通常是一条完整响应，同样没有包头：

```text
1F 79 11 44 50 33 30 35 50 2D 59 36 30 38 37 30 32 37 39 00 CC
└ 1F79   └ len=0x11  "DP305P-Y60870279\0"                    └ 校验
```

也就是 `Assembler::decode_one` 需要的命令区格式。一条命令可能被拆到多个
通知里，接收侧要先把通知内容拼进缓冲区，再逐条 `decode_one`。

### 流控：`ff03` 上的 `01 01` 信用

每向 `ff02` 写一次，模块就在 `ff03` 上回一个 `01 01`。

- 推荐 `write-without-response` + 在飞窗口：收到一个 `01 01` 才允许再发一个，
  窗口实测取 **8** 最快（32 反而更慢）。
- **不要**一口气灌几十 KB：写操作会全部返回成功，但模块会丢数据，
  表现为打印到一半就停。

### 带宽与画质

| 指标             | 实测                         |
| ---------------- | ---------------------------- |
| ATT MTU          | 515（单次可写 512 字节）     |
| 主机侧发送       | ~18 KiB/s                    |
| 打印机侧有效消费 | **~4.9 KB/s**                |

打印耗时几乎恒定，与速度档位无关，瓶颈在「BLE 模块 → 打印机串口」。
后果是走纸变慢、加热不连续，大面积黑块不均匀、细竖线发虚。
追求画质时用最慢速度 + 更高浓度，画质优先仍建议走 USB。

> 来源: [issue #3](https://github.com/sb-child/dz-print/issues/3) 里
> `clementatt` 的 DP305P 实测结论和 `feat/ble-backend` 实现。
> `ff10/ff11/ff12` 用途、`ff03` 的 `01 07` / `02 02 02` 帧仍待确认。

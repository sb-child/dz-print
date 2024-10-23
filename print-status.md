正常 0x00 = 0
[... 1f, 70, 08, 00, 01, 01, 00, 00, 35, 05, 10, 3b]
纸仓盒盖被打开 0x22 = 34
[... 1f, 70, 08, 22, 01, 01, 00, 00, 35, 05, 10, 19]
未检测到纸张 0x23 = 35
[... 1f, 70, 08, 23, 01, 01, 00, 00, 35, 05, 10, 18]
正在走纸 0x02 = 2
[... 1f, 70, 08, 02, 01, 01, 00, 00, 35, 05, 12, 37]
正在打印 0x01 = 1
[... 1f, 70, 08, 01, 01, 02, 00, 00, 0f, 05, 11, 5e]

[... 1f, 70, 08, 00, 01, 02, 00, 00, 63, 05, 10, 0c]
[... 1f, 70, 08, 00, 01, 01, 00, 00, 36, 05, 10, 3a]

```rust
pub enum PrinterErrorCode {
  Cancelled = 12,
  VolTooLow = 30,
  VolTooHigh = 31,
  TphNotFound = 32,
  TphTooHot = 33,
  CoverOpened = 34,
  NoPaper = 35,
  TphOpened = 36,
  NoRibbon = 37,
  UnmatchedRibbon = 38,
  TphTooCold = 39,
  UsedupRibbon = 40,
  UsedupRibbon2 = 41,
  LabelCanOpend = 50,
}
```

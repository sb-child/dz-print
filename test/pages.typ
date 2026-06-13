#set page(width: 48mm, height: auto, margin: (top: 5mm, bottom: 0.4mm, left: 0mm, right: 0mm))
#set text(font: "Unifont", size: 8pt)

#metadata((darkness: 9, speed: 2)) <print-settings>
= 测试文本喵喵喵
- darkness = 9
- speed = 2
// 这里会把p的尾巴截断所以留了0.4mm间距
#pagebreak()

#set page(width: 48mm, height: auto, margin: (top: 0mm, bottom: 0.4mm, left: 0mm, right: 0mm))
#metadata((darkness: "normal", speed: "normal")) <print-settings>
= 测试文本喵喵喵
- darkness = "normal" (6)
- speed = "normal" (3)
#pagebreak()

#set page(width: 48mm, height: auto, margin: (top: 0mm, bottom: 0.4mm, left: 0mm, right: 0mm))
#metadata((darkness: "min", speed: "min")) <print-settings>
= 测试文本喵喵喵
- darkness = "min" (1)
- speed = "min" (1)
#pagebreak()

#set page(width: 48mm, height: auto, margin: (top: 0mm, bottom: 5mm, left: 0mm, right: 0mm))
#metadata((darkness: "max", speed: "max")) <print-settings>
= 测试文本喵喵喵
- darkness = "max" (15)
- speed = "max" (5)

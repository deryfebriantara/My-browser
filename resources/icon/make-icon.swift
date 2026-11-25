// Menggambar ikon aplikasi my-browser (1024×1024 PNG) dengan CoreGraphics.
// Konsep: bola dunia (browser) di langit malam dengan bulan sabit (tab yang
// tidur, hemat memory). Pemakaian: swift make-icon.swift <keluaran.png>
import AppKit

let size: CGFloat = 1024
let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon-1024.png"

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(red: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
}

let space = CGColorSpaceCreateDeviceRGB()
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// Badan ikon: kotak membulat 824 pt dengan margin 100 pt (grid ikon macOS).
let body = CGRect(x: 100, y: 100, width: 824, height: 824)
let shape = CGPath(roundedRect: body, cornerWidth: 186, cornerHeight: 186, transform: nil)

// Bayangan halus di bawah badan.
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -14), blur: 30, color: color(0x000000, 0.35))
ctx.addPath(shape)
ctx.setFillColor(color(0x18225A))
ctx.fillPath()
ctx.restoreGState()

// Langit malam: gradasi biru dari atas ke bawah.
ctx.saveGState()
ctx.addPath(shape)
ctx.clip()
let sky = CGGradient(colorsSpace: space, colors: [color(0x3A58D6), color(0x1A2463)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(sky, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])
// Kilau lembut di atas.
let glow = CGGradient(colorsSpace: space, colors: [color(0xFFFFFF, 0.16), color(0xFFFFFF, 0)] as CFArray, locations: [0, 1])!
ctx.drawRadialGradient(glow, startCenter: CGPoint(x: 380, y: 860), startRadius: 0,
                       endCenter: CGPoint(x: 380, y: 860), endRadius: 520, options: [])

// Bintang.
for (x, y, r) in [(250.0, 780.0, 9.0), (330.0, 852.0, 6.0), (205.0, 650.0, 5.0), (820.0, 330.0, 6.0), (300.0, 300.0, 4.5)] {
    ctx.setFillColor(color(0xFFFFFF, 0.85))
    ctx.fillEllipse(in: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r))
}

// Bola dunia.
let center = CGPoint(x: 492, y: 468)
let radius: CGFloat = 248
ctx.setStrokeColor(color(0xFFFFFF))
ctx.setLineCap(.round)
ctx.setLineWidth(34)
ctx.strokeEllipse(in: CGRect(x: center.x - radius, y: center.y - radius, width: 2 * radius, height: 2 * radius))
ctx.saveGState()
ctx.addEllipse(in: CGRect(x: center.x - radius, y: center.y - radius, width: 2 * radius, height: 2 * radius))
ctx.clip()
ctx.setLineWidth(22)
ctx.setStrokeColor(color(0xFFFFFF, 0.92))
let meridian = radius * 0.5
ctx.strokeEllipse(in: CGRect(x: center.x - meridian, y: center.y - radius, width: 2 * meridian, height: 2 * radius))
ctx.move(to: CGPoint(x: center.x, y: center.y - radius)); ctx.addLine(to: CGPoint(x: center.x, y: center.y + radius))
for dy in [-radius * 0.5, 0, radius * 0.5] {
    ctx.move(to: CGPoint(x: center.x - radius, y: center.y + dy)); ctx.addLine(to: CGPoint(x: center.x + radius, y: center.y + dy))
}
ctx.strokePath()
ctx.restoreGState()

// Bulan sabit keemasan di kanan atas: lingkaran bulan dikurangi lingkaran
// pemotong, dengan cincin warna langit agar terpisah dari bola dunia.
let moon = CGPoint(x: 718, y: 718)
let moonR: CGFloat = 112
let cut = CGPoint(x: moon.x + 58, y: moon.y + 40)
func clipCrescent(_ grow: CGFloat) {
    ctx.addEllipse(in: CGRect(x: moon.x - moonR - grow, y: moon.y - moonR - grow,
                              width: 2 * (moonR + grow), height: 2 * (moonR + grow)))
    ctx.clip()
    ctx.addRect(CGRect(x: 0, y: 0, width: size, height: size))
    ctx.addEllipse(in: CGRect(x: cut.x - moonR + grow, y: cut.y - moonR + grow,
                              width: 2 * (moonR - grow), height: 2 * (moonR - grow)))
    ctx.clip(using: .evenOdd)
}
ctx.saveGState()
clipCrescent(20)
ctx.setFillColor(color(0x2A3888))
ctx.fill(CGRect(x: 0, y: 0, width: size, height: size))
ctx.restoreGState()
ctx.saveGState()
clipCrescent(0)
let gold = CGGradient(colorsSpace: space, colors: [color(0xFFDE94), color(0xF4A23C)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(gold, start: CGPoint(x: moon.x - moonR, y: moon.y + moonR),
                       end: CGPoint(x: moon.x + moonR, y: moon.y - moonR), options: [])
ctx.restoreGState()
ctx.restoreGState()

let image = ctx.makeImage()!
let rep = NSBitmapImageRep(cgImage: image)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: output))
print("ikon: \(output)")

import AppKit
import Foundation

// -----------------------------------------------------------------------------
// rdog display-aware E2E fixture
// -----------------------------------------------------------------------------

private struct FixtureConfig {
    let requiredDisplayCount: Int

    static func parse(arguments: [String]) -> FixtureConfig {
        guard let index = arguments.firstIndex(of: "--require-displays"),
              arguments.indices.contains(index + 1),
              let count = Int(arguments[index + 1]),
              count > 0 else {
            return FixtureConfig(requiredDisplayCount: 1)
        }
        return FixtureConfig(requiredDisplayCount: count)
    }
}

private final class FixtureController: NSObject, NSApplicationDelegate {
    private let config: FixtureConfig
    private var windows: [NSWindow] = []
    private var labels: [NSTextField] = []
    private var clickCounts: [Int] = []
    private var terminationSource: DispatchSourceSignal?

    init(config: FixtureConfig) {
        self.config = config
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let screens = NSScreen.screens.sorted {
            if $0.frame.minX == $1.frame.minX {
                return $0.frame.minY < $1.frame.minY
            }
            return $0.frame.minX < $1.frame.minX
        }

        guard screens.count >= config.requiredDisplayCount else {
            writeJSON([
                "status": "error",
                "error_code": "INSUFFICIENT_DISPLAYS",
                "required_display_count": config.requiredDisplayCount,
                "actual_display_count": screens.count,
            ])
            Foundation.exit(2)
        }

        let fixtureDisplayCount = min(max(config.requiredDisplayCount, 1), 2)
        for index in 0..<fixtureDisplayCount {
            createWindow(index: index, screen: screens[index])
        }

        NSApplication.shared.activate(ignoringOtherApps: true)
        windows.first?.makeKeyAndOrderFront(nil)
        installTerminationHandler()
        writeReadyPayload(screens: screens)
    }

    @objc private func handleButton(_ sender: NSButton) {
        let index = sender.tag
        guard clickCounts.indices.contains(index), labels.indices.contains(index) else {
            return
        }
        clickCounts[index] += 1
        labels[index].stringValue = "count:\(clickCounts[index])"
        labels[index].setAccessibilityLabel("display-aware-count-\(index + 1)")
        labels[index].setAccessibilityValue(labels[index].stringValue)
    }

    private func createWindow(index: Int, screen: NSScreen) {
        let visible = screen.visibleFrame
        let width = min(520.0, max(360.0, visible.width - 160.0))
        let height = min(320.0, max(240.0, visible.height - 160.0))
        let frame = NSRect(
            x: visible.minX + 80.0,
            y: visible.maxY - height - 80.0,
            width: width,
            height: height
        )
        let title = "rdog-display-aware-d\(index + 1)"
        let window = NSWindow(
            contentRect: frame,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false,
            screen: screen
        )
        window.title = title
        window.setFrame(frame, display: true)
        window.isReleasedWhenClosed = false
        window.setAccessibilityIdentifier("rdog-display-aware-window-\(index + 1)")

        let content = NSView(frame: NSRect(origin: .zero, size: frame.size))
        content.autoresizingMask = [.width, .height]

        let heading = NSTextField(labelWithString: "fixture-display:\(index + 1)")
        heading.frame = NSRect(x: 28, y: height - 64, width: width - 56, height: 28)
        heading.font = NSFont.boldSystemFont(ofSize: 18)
        heading.setAccessibilityIdentifier("display-aware-heading-\(index + 1)")
        heading.setAccessibilityLabel("display-aware-heading-\(index + 1)")
        content.addSubview(heading)

        let label = NSTextField(labelWithString: "count:0")
        label.frame = NSRect(x: 28, y: height - 112, width: width - 56, height: 28)
        label.setAccessibilityIdentifier("display-aware-count-\(index + 1)")
        label.setAccessibilityLabel("display-aware-count-\(index + 1)")
        label.setAccessibilityValue("count:0")
        content.addSubview(label)

        let input = NSTextField(string: "fixture-input-\(index + 1)")
        input.frame = NSRect(x: 28, y: height - 164, width: width - 56, height: 32)
        input.setAccessibilityIdentifier("display-aware-input-\(index + 1)")
        input.setAccessibilityLabel("display-aware-input-\(index + 1)")
        content.addSubview(input)

        let button = NSButton(title: "increment-display-\(index + 1)", target: self, action: #selector(handleButton(_:)))
        button.frame = NSRect(x: 28, y: 36, width: 220, height: 36)
        button.bezelStyle = .rounded
        button.tag = index
        button.setAccessibilityIdentifier("display-aware-button-\(index + 1)")
        button.setAccessibilityLabel("display-aware-button-\(index + 1)")
        content.addSubview(button)

        window.contentView = content
        window.orderFrontRegardless()
        windows.append(window)
        labels.append(label)
        clickCounts.append(0)
    }

    private func installTerminationHandler() {
        signal(SIGTERM, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        source.setEventHandler {
            NSApplication.shared.terminate(nil)
        }
        source.resume()
        terminationSource = source
    }

    private func writeReadyPayload(screens: [NSScreen]) {
        let windowPayload = windows.enumerated().map { index, window in
            [
                "title": window.title,
                "display_index": index + 1,
                "frame": rectValue(window.frame),
                "button_accessibility_id": "display-aware-button-\(index + 1)",
                "count_accessibility_id": "display-aware-count-\(index + 1)",
            ] as [String: Any]
        }
        let displayPayload = screens.enumerated().map { index, screen in
            [
                "index": index + 1,
                "frame": rectValue(screen.frame),
                "visible_frame": rectValue(screen.visibleFrame),
            ] as [String: Any]
        }
        writeJSON([
            "status": "ready",
            "pid": ProcessInfo.processInfo.processIdentifier,
            "display_count": screens.count,
            "windows": windowPayload,
            "displays": displayPayload,
        ])
    }

    private func rectValue(_ rect: NSRect) -> [String: Double] {
        [
            "x": rect.origin.x,
            "y": rect.origin.y,
            "width": rect.size.width,
            "height": rect.size.height,
        ]
    }

    private func writeJSON(_ value: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
              var text = String(data: data, encoding: .utf8) else {
            Foundation.exit(3)
        }
        text.append("\n")
        FileHandle.standardOutput.write(Data(text.utf8))
    }
}

private let config = FixtureConfig.parse(arguments: CommandLine.arguments)
let application = NSApplication.shared
private let controller = FixtureController(config: config)
application.setActivationPolicy(.regular)
application.delegate = controller
application.run()

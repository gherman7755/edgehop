import AppKit
import EdgeHopCore
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Sharing keeps running from the menu bar when the window is closed.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationWillTerminate(_ notification: Notification) {
        AppModel.shared.shutdown() // always give the cursor back
    }
}

@main
struct EdgeHopApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var model = AppModel.shared

    var body: some Scene {
        Window("EdgeHop", id: "main") {
            ContentView().environmentObject(model)
        }
        .windowResizability(.contentSize)

        MenuBarExtra("EdgeHop", systemImage: model.menuSymbol) {
            MenuContent().environmentObject(model)
        }
    }
}

struct MenuContent: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(model.statusText)
        Divider()
        Button("Show EdgeHop") {
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
        }
        if model.isRunning {
            Button(model.mode == .controller ? "Disconnect" : "Stop listening") {
                model.mode == .controller ? model.stopController() : model.stopReceiver()
            }
        }
        Divider()
        Button("Quit EdgeHop") { NSApp.terminate(nil) }
    }
}

struct ContentView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header
            Picker("", selection: $model.mode) {
                ForEach(AppMode.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .disabled(model.isRunning)

            permissions

            if model.mode == .controller { ControllerPanel() } else { ReceiverPanel() }

            logSection
            Text("Panic chord ⌃⌥⌘⎋ stops sharing on either computer. The link is encrypted; anyone who knows the token can connect, so keep it private.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(20)
        .frame(width: 500)
    }

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: model.menuSymbol)
                .font(.system(size: 28))
                .foregroundStyle(model.isRunning ? Color.accentColor : Color.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text("EdgeHop").font(.title2.bold())
                Text(model.statusText)
                    .font(.callout)
                    .foregroundStyle(model.statusIsError ? Color.red : Color.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            if let latency = model.latency {
                Text(String(format: "%.0f ms", latency)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            }
        }
    }

    private var permissions: some View {
        GroupBox("Permissions") {
            VStack(alignment: .leading, spacing: 8) {
                permissionRow(
                    ok: model.accessibilityOK, title: "Accessibility",
                    detail: "Lets EdgeHop move the cursor and type on this computer.", action: model.requestAccessibility
                )
                permissionRow(
                    ok: model.inputMonitoringOK, title: "Input Monitoring",
                    detail: "Lets EdgeHop see this computer's mouse and keyboard (and the panic chord).", action: model.requestInputMonitoring
                )
                if !(model.accessibilityOK && model.inputMonitoringOK) {
                    Text("After granting, quit and reopen EdgeHop once.").font(.caption).foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 4)
        }
    }

    private func permissionRow(ok: Bool, title: String, detail: String, action: @escaping () -> Void) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                .foregroundStyle(ok ? Color.green : Color.orange)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(.callout.weight(.medium))
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if !ok { Button("Grant…", action: action) }
        }
    }

    private var logSection: some View {
        DisclosureGroup("Log", isExpanded: $model.logExpanded) {
            VStack(alignment: .leading, spacing: 8) {
                Toggle("Verbose diagnostics (to help find problems)", isOn: $model.debugLog)
                    .disabled(model.isRunning)
                ScrollViewReader { proxy in
                    ScrollView {
                        Text(model.logLines.isEmpty ? "Nothing yet." : model.logLines.joined(separator: "\n"))
                            .font(.system(size: 11, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .id("end")
                    }
                    .frame(height: 170)
                    .background(Color(nsColor: .textBackgroundColor))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.3)))
                    .onChange(of: model.logLines.count) { _ in proxy.scrollTo("end", anchor: .bottom) }
                }
                HStack {
                    Button("Copy all") { model.copy(model.logLines.joined(separator: "\n")) }
                    Button("Clear") { model.clearLog() }
                }
            }
            .padding(.top, 6)
        }
    }
}

struct ControllerPanel: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        GroupBox("Control another computer") {
            VStack(alignment: .leading, spacing: 12) {
                VStack(alignment: .leading, spacing: 12) {
                    if model.peers.isEmpty {
                        Text("No computers found yet. On the other computer open EdgeHop, choose “Be controlled” and press Start.")
                            .font(.caption).foregroundStyle(.secondary)
                    } else {
                        Picker("Found nearby", selection: $model.selectedPeer) {
                            Text("Enter an address instead").tag(String?.none)
                            ForEach(model.peers) { peer in Text(peer.name).tag(Optional(peer.name)) }
                        }
                    }
                    TextField("Address, for example 192.168.1.20", text: $model.manualHost)
                        .textFieldStyle(.roundedBorder)
                        .disabled(model.selectedPeer != nil)

                    SecureField("Token shown on the other computer", text: $model.peerToken)
                        .textFieldStyle(.roundedBorder)

                    Picker("The other computer is", selection: $model.side) {
                        Text("Left").tag(Side.left)
                        Text("Right").tag(Side.right)
                        Text("Above").tag(Side.top)
                        Text("Below").tag(Side.bottom)
                    }
                    .pickerStyle(.segmented)

                    Toggle("Move the cursor across the screen edge (off: mirror the mouse on both)", isOn: $model.switching)
                    Toggle("Also share the keyboard (everything you type goes to the other computer)", isOn: $model.forwardKeys)
                }
                .disabled(model.isRunning) // settings are fixed while connected; Disconnect stays usable

                HStack {
                    Spacer()
                    if model.isRunning {
                        Button("Disconnect") { model.stopController() }
                            .keyboardShortcut(.cancelAction)
                    } else {
                        Button("Connect") { model.startController() }
                            .keyboardShortcut(.defaultAction)
                            .disabled(!model.canConnect)
                    }
                }
            }
            .padding(.vertical, 4)
        }
    }
}

struct ReceiverPanel: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        GroupBox("Be controlled") {
            VStack(alignment: .leading, spacing: 12) {
                Text("Enter this token on the computer that will control this one:")
                    .font(.callout)
                HStack {
                    Text(model.ownToken)
                        .font(.system(.title3, design: .monospaced))
                        .textSelection(.enabled)
                    Spacer()
                    Button("Copy") { model.copy(model.ownToken) }
                    Button("New token") { model.regenerateOwnToken() }
                        .disabled(model.isRunning)
                }
                Text("This computer appears as “\(Device.name)”.")
                    .font(.caption).foregroundStyle(.secondary)
                Toggle("Accept keyboard input too (lets the other computer type here)", isOn: $model.allowKeys)
                    .disabled(model.isRunning)
                HStack {
                    Spacer()
                    if model.isRunning {
                        Button("Stop listening") { model.stopReceiver() }
                    } else {
                        Button("Start") { model.startReceiver() }
                            .keyboardShortcut(.defaultAction)
                    }
                }
            }
            .padding(.vertical, 4)
        }
    }
}

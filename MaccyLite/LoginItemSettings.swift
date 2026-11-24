import AppKit
import ServiceManagement
import SwiftUI

struct LoginItemSettings: View {
  @State private var status = SMAppService.mainApp.status
  @State private var error: String?

  var body: some View {
    Section("Startup") {
      Toggle("Launch at Login", isOn: Binding(
        get: { status == .enabled || status == .requiresApproval },
        set: { enabled in setEnabled(enabled) }
      ))
      if status == .requiresApproval {
        Text("Allow Maccy Lite in System Settings to finish enabling startup.")
        Button("Open Login Items Settings") { SMAppService.openSystemSettingsLoginItems() }
      }
      if let error { Text(error).foregroundStyle(.red) }
    }
    .onAppear { status = SMAppService.mainApp.status }
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      status = SMAppService.mainApp.status
    }
  }

  private func setEnabled(_ enabled: Bool) {
    defer { status = SMAppService.mainApp.status }
    error = nil
    do {
      if enabled {
        try SMAppService.mainApp.register()
      } else {
        try SMAppService.mainApp.unregister()
      }
    } catch {
      if !enabled || SMAppService.mainApp.status != .requiresApproval {
        self.error = "macOS could not change the login item. Use the app from a permanent folder, then try again."
      }
    }
  }
}

// This file is created by Veyra and will not be overwritten

import Foundation

@MainActor
final class SettingsViewModel: SettingsViewModelContract {
    let state = SettingsState()

    func setNickname(_ value: String) {
        // Keep nicknames short
        state.nickname = String(value.prefix(20))
    }

    func save() async {
        UserDefaults.standard.set(state.reminder, forKey: "reminder")
        UserDefaults.standard.set(state.volume, forKey: "volume")
        UserDefaults.standard.set(state.nickname, forKey: "nickname")
    }
}

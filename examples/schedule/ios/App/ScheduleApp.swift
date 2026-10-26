import SwiftData
import SwiftUI

@MainActor
final class InMemoryCourseRepository: CourseRepository {
    private var courses: [Course] = [
        Course(id: "1", name: "Compilers", teacher: "Aho", credits: 4, kind: .lecture),
        Course(id: "2", name: "Operating Systems", teacher: nil, credits: 3, kind: .lab),
    ]

    func getCourses() async throws -> [Course] {
        courses
    }

    func observeCourses() -> AsyncStream<[Course]> {
        let snapshot = courses
        return AsyncStream { continuation in
            continuation.yield(snapshot)
            continuation.finish()
        }
    }
}

@MainActor
final class AppDependencies: VeyraDependencies {
    let container: ModelContainer
    let stores: VeyraStores
    let courseRepository: any CourseRepository = InMemoryCourseRepository()

    let appPreferences: any AppPreferences = UserDefaultsAppPreferences()
    let account: any Account = KeychainAccount()

    var noteStore: any NoteStore { stores.noteStore }

    init() {
        container = try! VeyraStorage.makeContainer()
        stores = VeyraStores(context: container.mainContext)
    }
}

@main
struct ScheduleApp: App {
    private let dependencies = AppDependencies()

    var body: some Scene {
        WindowGroup {
            TabView {
                ScheduleRoot()
                    .tabItem { Label(L10n.scheduleTitle, systemImage: "calendar") }
                NotesRoot()
                    .tabItem { Label(L10n.notesAdd, systemImage: "note.text") }
                SettingsRoot()
                    .tabItem { Label(L10n.settingsTitle, systemImage: "gear") }
            }
            .veyraDependencies(dependencies)
        }
    }
}

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

/// 课程详情，由ScheduleRoot发出的showCourse事件导航到这里
struct CourseDetailView: View {
    let id: String
    let repository: any CourseRepository
    @State private var course: Course?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let course {
                Text(course.name).font(.largeTitle)
                Text(course.teacher ?? "-").foregroundStyle(.secondary)
                Text(course.credits.formatted(.number))
            } else {
                ProgressView()
            }
        }
        .padding()
        .veyraGlassEffect()
        .task {
            course = try? await repository.getCourses().first { $0.id == id }
        }
    }
}

@main
struct ScheduleApp: App {
    private let dependencies = AppDependencies()
    @State private var path: [String] = []

    var body: some Scene {
        WindowGroup {
            TabView {
                NavigationStack(path: $path) {
                    ScheduleRoot(onEffect: handle)
                        .navigationDestination(for: String.self) { id in
                            CourseDetailView(id: id, repository: dependencies.courseRepository)
                        }
                }
                .tabItem { Label(L10n.scheduleTitle, systemImage: "calendar") }
                NotesRoot()
                    .tabItem { Label(L10n.notesAdd, systemImage: "note.text") }
                SettingsRoot()
                    .tabItem { Label(L10n.settingsTitle, systemImage: "gear") }
            }
            .veyraDependencies(dependencies)
        }
    }

    /// ScheduleViewModel发出的一次性事件
    private func handle(_ effect: ScheduleEffect) {
        switch effect {
        case .showCourse(let id):
            path.append(id)
        }
    }
}

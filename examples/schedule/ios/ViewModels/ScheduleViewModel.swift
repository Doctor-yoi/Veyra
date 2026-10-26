// This file is created by Veyra and will not be overwritten

import Foundation

@MainActor
final class ScheduleViewModel: ScheduleViewModelContract {
    let state = ScheduleState()

    private let courses: any CourseRepository

    init(courses: any CourseRepository) {
        self.courses = courses
    }

    func refresh() async {
        state.isRefreshing = true
        defer { state.isRefreshing = false }
        do {
            let all = try await courses.getCourses()
            let query = state.query
            state.items = query.isEmpty ? all : all.filter { $0.name.localizedStandardContains(query) }
            state.errorMessage = nil
        } catch {
            state.errorMessage = error.localizedDescription
        }
    }

    func openCourse(id: String) {
        // Navigation is handled by the app
    }
}

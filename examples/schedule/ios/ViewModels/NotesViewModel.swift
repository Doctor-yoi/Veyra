// This file is created by Veyra and will not be overwritten

import Foundation

@MainActor
final class NotesViewModel: NotesViewModelContract {
    let state = NotesState()

    private let notes: any NoteStore

    init(notes: any NoteStore) {
        self.notes = notes
    }

    func start() async {
        for await items in notes.forCourse(course: state.courseId) {
            state.items = items
        }
    }

    func add() async {
        let note = Note(id: UUID().uuidString, courseId: state.courseId, text: state.draft, pinned: false, createdAt: .now)
        try? await notes.save(note)
        state.draft = ""
    }

    func remove(id: String) async {
        try? await notes.delete(id: id)
    }
}

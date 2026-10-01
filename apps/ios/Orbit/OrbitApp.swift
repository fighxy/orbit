import OrbitClient
import SwiftUI
import UIKit

@main
struct OrbitApp: App {
    var body: some Scene {
        WindowGroup {
            ComposeRoot()
                // Compose handles the keyboard and safe-area insets itself.
                .ignoresSafeArea()
        }
    }
}

/// Hosts the shared Compose UI from the Kotlin framework.
struct ComposeRoot: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UIViewController {
        MainViewControllerKt.MainViewController()
    }

    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}

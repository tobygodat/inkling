import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     configurationForConnecting session: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let config = UISceneConfiguration(name: nil, sessionRole: session.role)
        config.delegateClass = SceneDelegate.self
        return config
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession,
               options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        window.rootViewController = SpikeController()
        window.makeKeyAndVisible()
        self.window = window
    }

    func sceneDidBecomeActive(_ scene: UIScene) { gpui_ios_did_become_active(nil) }
    func sceneWillResignActive(_ scene: UIScene) { gpui_ios_will_resign_active(nil) }
}

/// UIKit owns this view's frame. GPUI owns what is drawn inside it.
final class GPUIView: UIView {
    private let gpuiWindow: UnsafeMutableRawPointer
    let content: UIViewController

    override init(frame: CGRect) {
        gpui_ios_set_embedded()
        gpui_ios_register_app()
        gpui_ios_run_demo()
        guard let window = gpui_ios_get_window(),
              let controller = gpui_ios_view_controller(window) else {
            fatalError("GPUI did not open its window")
        }
        gpuiWindow = window
        content = Unmanaged<UIViewController>.fromOpaque(controller).takeUnretainedValue()
        super.init(frame: frame)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { fatalError("unsupported") }

    func attach(to parent: UIViewController) {
        parent.addChild(content)
        addSubview(content.view)
        content.didMove(toParent: parent)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard bounds.width > 0, bounds.height > 0, content.view.frame != bounds else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        content.view.frame = bounds
        gpui_ios_layout_view(gpuiWindow)
        drawFrame()
        CATransaction.commit()
    }

    func drawFrame() { _ = gpui_ios_request_frame(gpuiWindow) }
}

final class SpikeController: UIViewController {
    private var gpui: GPUIView!
    private var displayLink: CADisplayLink?

    override func viewDidLoad() {
        super.viewDidLoad()
        overrideUserInterfaceStyle = .light
        view.backgroundColor = UIColor(red: 0xF4 / 255, green: 0xF5 / 255, blue: 0xF1 / 255, alpha: 1)
        gpui = GPUIView(frame: .zero)
        gpui.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(gpui)
        gpui.attach(to: self)
        NSLayoutConstraint.activate([
            gpui.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            gpui.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            gpui.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            gpui.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
        ])
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        displayLink = CADisplayLink(target: self, selector: #selector(tick))
        displayLink?.add(to: .main, forMode: .common)
    }

    override func viewWillDisappear(_ animated: Bool) {
        displayLink?.invalidate()
        displayLink = nil
        super.viewWillDisappear(animated)
    }

    @objc private func tick() { gpui.drawFrame() }
}

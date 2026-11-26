import AuthenticationServices
import UIKit

/// The host's side of Sign in with Google on iPhone: the system sign-in sheet
/// (`ASWebAuthenticationSession`) over the server's sign-in page, coming back on `inkling://`.
/// The Rust library (`ios/src/lib.rs`) asks with a token and is answered through
/// `inkling_ios_signed_in`.
@MainActor
final class SignIn: NSObject, ASWebAuthenticationPresentationContextProviding {
    static let shared = SignIn()

    weak var presenter: UIViewController?
    private var session: ASWebAuthenticationSession?

    func start(url: URL, token: UInt64) {
        #if DEBUG
        // A headless run signs in to the fake server: no sheet, and the fake's code comes straight
        // back, as the sheet would bring it.
        if getenv("INKLING_FAKE_AI") != nil {
            NSLog("INKLING SIGN-IN %@", url.absoluteString)
            let state = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "state" }?.value ?? ""
            var back = URLComponents(string: "inkling://callback")!
            back.queryItems = [URLQueryItem(name: "code", value: "fake-code"), URLQueryItem(name: "state", value: state)]
            answer(token, back.url)
            return
        }
        #endif
        session?.cancel()
        let session = ASWebAuthenticationSession(url: url, callback: .customScheme("inkling")) { [weak self] came, _ in
            self?.answer(token, came)
            self?.session = nil
        }
        session.presentationContextProvider = self
        self.session = session
        if !session.start() { answer(token, nil) }
    }

    private func answer(_ token: UInt64, _ url: URL?) {
        if let url {
            url.absoluteString.withCString { inkling_ios_signed_in(token, $0) }
        } else {
            inkling_ios_signed_in(token, nil)
        }
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        presenter?.view.window ?? ASPresentationAnchor()
    }
}

// Rust calls this on the main thread, where GPUI runs, and `assumeIsolated` traps if that ever
// changes. The sheet answers later through `inkling_ios_signed_in`.
@_cdecl("inkling_host_sign_in")
func inklingHostSignIn(_ token: UInt64, _ url: UnsafePointer<CChar>) {
    let address = String(cString: url)
    MainActor.assumeIsolated {
        guard let url = URL(string: address) else { return inkling_ios_signed_in(token, nil) }
        SignIn.shared.start(url: url, token: token)
    }
}

import AuthenticationServices
import Foundation
import InklingCore
import Observation
import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Account sign-in, background sync, RSS/Atom feeds and Readwise over `inkling-sync` (ADR 0004,
/// ADR 0007, TOB-556). Runs sync at launch, when the app becomes active, 3 s after a local change
/// settles and every 5 minutes while open. Nothing shows while sync works; any failure sets
/// `syncStopped` until the next sync succeeds.
@Observable
final class Account {
    private static let settleDelay: Duration = .seconds(3)
    private static let pollInterval: Duration = .seconds(5 * 60)

    let client: SyncClient
    @ObservationIgnored private weak var shelf: Shelf?

    private(set) var signedIn: Bool
    private(set) var signingIn = false
    private(set) var syncing = false
    private(set) var syncStopped = false
    private(set) var readwiseConnected = false
    private(set) var readwiseBusy = false
    private(set) var feeds: [FeedInfo] = []
    private(set) var feedsLoading = false
    private(set) var feedBusy = false
    var copiedAgents = false
    var copiedNewsletter = false
    var problem: String?
    var feedProblem: String?

    @ObservationIgnored private var again = false
    @ObservationIgnored private var syncTask: Task<Void, Never>?
    @ObservationIgnored private var settleTask: Task<Void, Never>?
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var signInTask: Task<Void, Never>?
    @ObservationIgnored private var activeSession: SignInSession?
    #if os(iOS)
    @ObservationIgnored private var webAuth: WebAuthPresenter?
    #endif

    init(client: SyncClient) {
        self.client = client
        self.signedIn = client.isSignedIn()
    }

    func attach(shelf: Shelf) {
        self.shelf = shelf
    }

    var agentsAddress: String? {
        client.agentsAddress()
    }

    // MARK: - Background Sync

    /// Starts the repeating sync loop and runs one sync right away if signed in.
    func startBackgroundSync() {
        guard pollTask == nil else { return }
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                if let self, self.signedIn {
                    self.syncNow()
                }
                try? await Task.sleep(for: Self.pollInterval)
            }
        }
    }

    /// Schedules a sync 3 seconds after the last local edit settles.
    func syncSoon() {
        guard signedIn else { return }
        settleTask?.cancel()
        settleTask = Task { [weak self] in
            try? await Task.sleep(for: Self.settleDelay)
            guard !Task.isCancelled else { return }
            self?.syncNow()
        }
    }

    /// Triggers a sync now, or queues one more pass when a sync is already running.
    func syncNow() {
        guard signedIn else { return }
        if syncing {
            again = true
            return
        }
        syncTask = Task { [weak self] in
            await self?.runSyncPass()
        }
    }

    /// Runs a sync pass and awaits its completion (used by pull-to-refresh and tests).
    @discardableResult
    func syncNowAsync() async -> SyncReport? {
        guard signedIn else { return nil }
        if let existing = syncTask, syncing {
            again = true
            await existing.value
            if let followUp = syncTask, syncing {
                await followUp.value
            }
            return nil
        }
        return await runSyncPass()
    }

    @discardableResult
    private func runSyncPass() async -> SyncReport? {
        guard signedIn, let shelf else { return nil }
        if syncing {
            again = true
            return nil
        }
        syncing = true
        shelf.flushPlaces(triggerSync: false)
        shelf.activeWriter?.saveNow(triggerSync: false)

        let client = client
        var report: SyncReport?
        do {
            let pending = try await Shelf.offMain { try client.fetch() }
            // Read active/held document state AFTER fetch returns so typing or review started
            // during the network call is never clobbered.
            let openDoc = shelf.activeWriter?.id
            var heldDocs: [String] = []
            if let writer = shelf.activeWriter, writer.saved != .saved || writer.isAsking || writer.isReviewing {
                heldDocs.append(writer.id)
            }
            let applied = try client.apply(
                pending: pending,
                library: shelf.library,
                documents: shelf.documents,
                openDoc: openDoc,
                heldDocs: heldDocs
            )
            report = applied
            syncStopped = false
            readwiseConnected = applied.readwiseConnected
            shelf.reload()
            shelf.reloadDocuments()
            if let writer = shelf.activeWriter, applied.touchedDocs.contains(writer.id) {
                writer.reloadFromSync(shelf.readDocument(writer.id))
            }
            if applied.importedCount > 0 {
                // Upload newly parsed articles right away so other devices download them without re-parsing.
                syncSoon()
            }
        } catch let error as InklingError {
            if case .SignedOut = error {
                signedIn = false
                readwiseConnected = false
                feeds = []
                syncStopped = true
            } else {
                syncStopped = true
            }
        } catch {
            syncStopped = true
        }

        syncing = false
        syncTask = nil
        if again && signedIn {
            again = false
            syncNow()
        }
        return report
    }

    // MARK: - Google Sign-In & Sign-Out

    func signIn() {
        signInTask?.cancel()
        activeSession?.cancel()
        activeSession = nil
        signInTask = Task { [weak self] in
            await self?.signInAsync()
        }
    }

    @discardableResult
    func signInAsync() async -> Bool {
        activeSession?.cancel()
        activeSession = nil
        signingIn = true
        problem = nil
        defer {
            signingIn = false
            activeSession = nil
        }

        let client = client
        do {
            #if os(macOS)
            let session = try await Shelf.offMain { try client.beginLoopbackSignIn(clientName: "Inkling for Mac") }
            activeSession = session
            if client.isHeadless(), let callback = client.fakeCallbackUrl(authorizeUrl: session.url()) {
                try await Shelf.offMain { try client.finishSignIn(session: session, callbackUrl: callback) }
            } else {
                guard let url = URL(string: session.url()) else {
                    throw InklingError.Sync(message: "The server’s sign-in address wasn’t valid.")
                }
                NSWorkspace.shared.open(url)
                try await Shelf.offMain { try client.waitAndFinishLoopback(session: session) }
            }
            #else
            let session = try await Shelf.offMain { try client.beginSignIn(redirectUri: "inkling://callback", clientName: "Inkling for iPhone") }
            activeSession = session
            let callback: String
            if client.isHeadless(), let fake = client.fakeCallbackUrl(authorizeUrl: session.url()) {
                callback = fake
            } else {
                guard let url = URL(string: session.url()) else {
                    throw InklingError.Sync(message: "The server’s sign-in address wasn’t valid.")
                }
                let presenter = WebAuthPresenter()
                webAuth = presenter
                defer { webAuth = nil }
                callback = try await presenter.authenticate(url: url, scheme: "inkling")
            }
            try await Shelf.offMain { try client.finishSignIn(session: session, callbackUrl: callback) }
            #endif

            signedIn = true
            syncStopped = false
            problem = nil
            await runSyncPass()
            await refreshFeeds()
            return true
        } catch let error as InklingError {
            if case .Cancelled = error {
                return false
            }
            problem = Shelf.words(error)
            return false
        } catch is CancellationError {
            return false
        } catch {
            problem = Shelf.words(error)
            return false
        }
    }

    func signOut() {
        signInTask?.cancel()
        activeSession?.cancel()
        activeSession = nil
        settleTask?.cancel()
        settleTask = nil
        again = false
        do {
            try client.signOut()
            signedIn = false
            syncStopped = false
            readwiseConnected = false
            feeds = []
            copiedAgents = false
            copiedNewsletter = false
            problem = nil
            feedProblem = nil
        } catch {
            problem = Shelf.words(error)
        }
    }

    /// Where newsletters are sent, while signed in.
    var newsletterAddress: String? {
        client.newsletterAddress()
    }

    func copyAgentsAddress() {
        guard let address = agentsAddress else { return }
        Self.copy(address)
        copiedAgents = true
    }

    func copyNewsletterAddress() {
        guard let address = newsletterAddress else { return }
        Self.copy(address)
        copiedNewsletter = true
    }

    private static func copy(_ text: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #else
        UIPasteboard.general.string = text
        #endif
    }

    // MARK: - Readwise (token goes only to the server, never stored on device)

    @discardableResult
    func connectReadwise(token: String) async -> Bool {
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            problem = "Paste the token from readwise.io/access_token."
            return false
        }
        readwiseBusy = true
        problem = nil
        defer { readwiseBusy = false }
        let client = client
        do {
            readwiseConnected = try await Shelf.offMain { try client.connectReadwise(token: trimmed) }
            return readwiseConnected
        } catch let error as InklingError {
            handleAccountError(error)
            return false
        } catch {
            problem = Shelf.words(error)
            return false
        }
    }

    func disconnectReadwise() async {
        readwiseBusy = true
        problem = nil
        defer { readwiseBusy = false }
        let client = client
        do {
            readwiseConnected = try await Shelf.offMain { try client.disconnectReadwise() }
        } catch let error as InklingError {
            handleAccountError(error)
        } catch {
            problem = Shelf.words(error)
        }
    }

    // MARK: - Feeds

    func refreshFeeds() async {
        guard signedIn else {
            feeds = []
            return
        }
        feedsLoading = true
        feedProblem = nil
        defer { feedsLoading = false }
        let client = client
        do {
            feeds = try await Shelf.offMain { try client.feeds() }
        } catch let error as InklingError {
            if case .SignedOut = error {
                handleAccountError(error)
            } else {
                feedProblem = Shelf.words(error)
            }
        } catch {
            feedProblem = Shelf.words(error)
        }
    }

    @discardableResult
    func followFeed(url: String) async -> Bool {
        let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        let normalized = trimmed.contains("://") ? trimmed : "https://\(trimmed)"
        guard let parsed = URL(string: normalized), parsed.scheme == "https" || parsed.scheme == "http", parsed.host() != nil else {
            feedProblem = "“\(trimmed)” isn’t a web address."
            return false
        }
        feedBusy = true
        feedProblem = nil
        defer { feedBusy = false }
        let client = client
        do {
            let added = try await Shelf.offMain { try client.followFeed(url: normalized) }
            if !feeds.contains(where: { $0.id == added.id }) {
                feeds.append(added)
                feeds.sort { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
            }
            return true
        } catch let error as InklingError {
            if case .SignedOut = error {
                handleAccountError(error)
            } else {
                feedProblem = Shelf.words(error)
            }
            return false
        } catch {
            feedProblem = Shelf.words(error)
            return false
        }
    }

    func unfollowFeed(_ feed: FeedInfo) async {
        feedBusy = true
        feedProblem = nil
        defer { feedBusy = false }
        let client = client
        let id = feed.id
        do {
            try await Shelf.offMain { try client.unfollowFeed(id: id) }
            feeds.removeAll { $0.id == id }
        } catch let error as InklingError {
            if case .SignedOut = error {
                handleAccountError(error)
            } else {
                feedProblem = Shelf.words(error)
            }
        } catch {
            feedProblem = Shelf.words(error)
        }
    }

    private func handleAccountError(_ error: InklingError) {
        if case .SignedOut = error {
            signedIn = false
            readwiseConnected = false
            feeds = []
            syncStopped = true
        }
        problem = Shelf.words(error)
    }
}

#if os(iOS)
/// Presents `ASWebAuthenticationSession` over the active window scene on iPhone.
private final class WebAuthPresenter: NSObject, ASWebAuthenticationPresentationContextProviding {
    private var session: ASWebAuthenticationSession?

    func authenticate(url: URL, scheme: String) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            let authSession = ASWebAuthenticationSession(url: url, callback: .customScheme(scheme)) { [weak self] callbackURL, error in
                Task { @MainActor in
                    self?.session = nil
                    if let callbackURL {
                        continuation.resume(returning: callbackURL.absoluteString)
                    } else if let authError = error as? ASWebAuthenticationSessionError, authError.code == .canceledLogin {
                        continuation.resume(throwing: InklingError.Cancelled)
                    } else if let error {
                        continuation.resume(throwing: InklingError.Sync(message: error.localizedDescription))
                    } else {
                        continuation.resume(throwing: InklingError.Cancelled)
                    }
                }
            }
            authSession.presentationContextProvider = self
            self.session = authSession
            if !authSession.start() {
                self.session = nil
                continuation.resume(throwing: InklingError.Sync(message: "Sign-in couldn’t open."))
            }
        }
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        if let window = scenes.flatMap(\.windows).first(where: \.isKeyWindow) ?? scenes.first?.windows.first {
            return window
        }
        return UIWindow(windowScene: scenes[0])
    }
}
#endif

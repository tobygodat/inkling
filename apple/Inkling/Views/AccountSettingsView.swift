import InklingCore
import SwiftUI

/// Account sign-in, sync status, Readwise and MCP Agents address. Shared between Mac Settings and
/// the iPhone Settings sheet.
struct AccountSettingsView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    @State private var showingReadwiseField = false
    @State private var readwiseToken = ""
    @FocusState private var readwiseFocused: Bool
    /// On iPhone, Settings puts Feeds right beside Readwise in one grouped form.
    var includeFeeds = false

    private var account: Account { shelf.account }

    var body: some View {
        if !account.signedIn {
            signedOutView
        } else {
            Form {
                if account.syncStopped {
                    SwiftUI.Section {
                        HStack {
                            HStack(spacing: 8) {
                                Circle()
                                    .fill(.primary)
                                    .frame(width: 6, height: 6)
                                Text("Sync stopped")
                            }
                            Spacer()
                            Button("Try again") { account.syncNow() }
                                #if os(iOS)
                                .foregroundStyle(Color(palette.surface))
                                .tint(Color(palette.ink))
                                #endif
                                .buttonStyle(.borderedProminent)
                                .accessibilityIdentifier("sync-try-again")
                        }
                    }
                    .settingsCard(palette)
                }

                SwiftUI.Section {
                    LabeledContent("Google") {
                        HStack(spacing: 12) {
                            if account.syncing {
                                ProgressView()
                                    .controlSize(.small)
                            }
                            Button("Sign out") { account.signOut() }
                                .accessibilityIdentifier("sign-out")
                        }
                    }
                    LabeledContent("Sync") {
                        Button(account.syncing ? "Syncing…" : "Sync Now") { account.syncNow() }
                            .disabled(account.syncing)
                            .accessibilityIdentifier("sync-now")
                    }
                } header: {
                    palette.caption("Account")
                }
                .settingsCard(palette)

                SwiftUI.Section {
                    if account.readwiseConnected {
                        LabeledContent {
                            Button("Disconnect") {
                                Task { await account.disconnectReadwise() }
                            }
                            .disabled(account.readwiseBusy)
                            .accessibilityIdentifier("readwise-disconnect")
                        } label: {
                            Label("Connected", systemImage: "checkmark.circle.fill")
                        }
                    } else if showingReadwiseField {
                        HStack(spacing: 8) {
                            SecureField("", text: $readwiseToken, prompt: palette.prompt("Readwise access token"))
                                .labelsHidden()
                                .autocorrectionDisabled()
                                #if os(iOS)
                                .textInputAutocapitalization(.never)
                                #endif
                                .focused($readwiseFocused)
                                .onSubmit { connectReadwise() }
                                .accessibilityIdentifier("readwise-token-field")
                            Button {
                                connectReadwise()
                            } label: {
                                Text("Connect")
                                    #if os(iOS)
                                    .foregroundStyle(Color(palette.surface))
                                    #endif
                            }
                            .buttonStyle(.borderedProminent)
                            #if os(iOS)
                            .tint(Color(palette.ink))
                            #endif
                            .disabled(readwiseToken.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || account.readwiseBusy)
                            .accessibilityIdentifier("readwise-submit")
                        }
                    } else {
                        LabeledContent("Readwise") {
                            Button("Connect") {
                                account.problem = nil
                                showingReadwiseField = true
                            }
                            .accessibilityIdentifier("readwise-connect")
                        }
                    }
                } header: {
                    palette.caption("Readwise")
                } footer: {
                    palette.caption("Your access token from readwise.io/access_token goes straight to your Inkling server and is never stored on this device.")
                }
                .settingsCard(palette)

                if includeFeeds {
                    FeedsFormSections()
                }

                SwiftUI.Section {
                    LabeledContent("Agents") {
                        Button(account.copiedAgents ? "Copied" : "Copy address") {
                            account.copyAgentsAddress()
                        }
                        .disabled(account.agentsAddress == nil)
                        .accessibilityIdentifier("copy-agents-address")
                    }
                } footer: {
                    palette.caption("Connect Claude or ChatGPT to your library and drafts over MCP.")
                }
                .settingsCard(palette)

                if let problem = account.problem {
                    SwiftUI.Section {
                        Text(problem)
                            .font(.inter(12, relativeTo: .caption))
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("account-problem")
                    }
                    .settingsCard(palette)
                }
            }
            .settingsForm(palette)
            .scrollDismissesKeyboard(.interactively)
            .task {
                if includeFeeds {
                    await account.refreshFeeds()
                }
            }
        }
    }

    private var signedOutView: some View {
        VStack(spacing: 18) {
            Image(systemName: "person.crop.circle")
                .font(.system(size: 40, weight: .light))
                .foregroundStyle(.secondary)

            VStack(spacing: 6) {
                Text("Sign in to Inkling")
                    .font(.serif(22, weight: .semibold, relativeTo: .title2))
                Text("Sync documents, books, reading places and highlights across your Mac and iPhone, follow RSS feeds, and connect Readwise.")
                    .font(.inter(13, relativeTo: .callout))
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 340)
            }

            Button {
                account.signIn()
            } label: {
                HStack(spacing: 8) {
                    if account.signingIn {
                        ProgressView()
                            .controlSize(.small)
                    }
                    Text("Sign in with Google")
                        .font(.inter(15, weight: .semibold, relativeTo: .body))
                }
                .foregroundStyle(Color(palette.surface))
                #if os(macOS)
                .padding(.horizontal, 20)
                .padding(.vertical, 10)
                .background(Capsule().fill(Color(palette.ink)))
                #else
                .padding(.horizontal, 12)
                .frame(minHeight: 32)
                #endif
            }
            #if os(macOS)
            .buttonStyle(.plain)
            #else
            .buttonStyle(.borderedProminent)
            .tint(Color(palette.ink))
            .controlSize(.large)
            #endif
            .disabled(account.signingIn)
            .accessibilityIdentifier("sign-in-google")

            if let problem = account.problem {
                Text(problem)
                    .font(.inter(12, relativeTo: .caption))
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 340)
                    .accessibilityIdentifier("account-problem")
            }
        }
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func connectReadwise() {
        let token = readwiseToken
        readwiseFocused = false
        Task {
            let ok = await account.connectReadwise(token: token)
            if ok {
                readwiseToken = ""
                showingReadwiseField = false
            }
        }
    }
}

/// Reusable Form sections for following and unfollowing RSS and Atom feeds.
struct FeedsFormSections: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    @State private var feedURL = ""
    @FocusState private var fieldFocused: Bool

    private var account: Account { shelf.account }

    var body: some View {
        SwiftUI.Section {
            HStack(spacing: 8) {
                TextField("", text: $feedURL, prompt: palette.prompt("Feed or site URL"))
                    .labelsHidden()
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    #endif
                    .focused($fieldFocused)
                    .onSubmit { follow() }
                    .accessibilityIdentifier("feed-url-field")
                Button {
                    follow()
                } label: {
                    Text("Follow")
                        #if os(iOS)
                        .foregroundStyle(feedURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? Color(palette.muted) : Color(palette.surface))
                        #endif
                }
                .buttonStyle(.borderedProminent)
                #if os(iOS)
                .tint(Color(palette.ink))
                #endif
                .disabled(feedURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || account.feedBusy)
                .accessibilityIdentifier("follow-feed")
            }
            if let problem = account.feedProblem {
                Text(problem)
                    .font(.inter(12, relativeTo: .caption))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("feed-problem")
            }
        } header: {
            palette.caption("Follow a Feed")
        } footer: {
            palette.caption("Paste an RSS or Atom feed address, or a site’s address to discover its feed.")
        }
        .settingsCard(palette)

        SwiftUI.Section {
            if account.feeds.isEmpty {
                Text(account.feedsLoading ? "Loading feeds…" : "No feeds followed yet.")
                    .font(.inter(13, relativeTo: .callout))
                    .foregroundStyle(.secondary)
            } else {
                ForEach(account.feeds, id: \.id) { feed in
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(feed.title)
                                .font(.inter(14, weight: .medium, relativeTo: .body))
                                .lineLimit(1)
                            Text(feed.url)
                                .font(.inter(12, relativeTo: .caption))
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            if let error = feed.lastError, !error.isEmpty {
                                Text(error)
                                    .font(.inter(11, relativeTo: .caption2))
                                    .foregroundStyle(.secondary)
                                    .lineLimit(2)
                            }
                        }
                        Spacer(minLength: 8)
                        Button("Unfollow") {
                            Task { await account.unfollowFeed(feed) }
                        }
                        .disabled(account.feedBusy)
                        .accessibilityIdentifier("unfollow-feed-\(feed.title)")
                    }
                    #if os(iOS)
                    .swipeActions(edge: .trailing) {
                        Button("Unfollow", role: .destructive) {
                            Task { await account.unfollowFeed(feed) }
                        }
                    }
                    #endif
                }
            }
        } header: {
            palette.caption("Followed Feeds")
        } footer: {
            palette.caption("New posts arrive in your Inbox on sync. Unfollowing keeps articles already in your library.")
        }
        .settingsCard(palette)
    }

    private func follow() {
        let url = feedURL
        fieldFocused = false
        Task {
            if await account.followFeed(url: url) {
                feedURL = ""
            }
        }
    }
}

/// Dedicated Feeds settings screen (Mac Settings "Feeds" tab).
struct FeedsSettingsView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette

    var body: some View {
        if !shelf.account.signedIn {
            ContentUnavailableView {
                Label("Sign in to follow feeds", systemImage: "dot.radiowaves.up.forward")
            } description: {
                Text("RSS and Atom feeds are polled by your Inkling server and arrive in your Inbox.")
            } actions: {
                Button {
                    shelf.account.signIn()
                } label: {
                    Text("Sign in with Google")
                        .foregroundStyle(Color(palette.surface))
                        #if os(macOS)
                        .font(.inter(14, weight: .semibold, relativeTo: .body))
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                        .background(Capsule().fill(Color(palette.ink)))
                        #endif
                }
                #if os(macOS)
                .buttonStyle(.plain)
                #else
                .buttonStyle(.borderedProminent)
                .tint(Color(palette.ink))
                #endif
                .disabled(shelf.account.signingIn)
            }
            .padding(24)
        } else {
            Form {
                FeedsFormSections()
            }
            .settingsForm(palette)
            .scrollDismissesKeyboard(.interactively)
            .task { await shelf.account.refreshFeeds() }
        }
    }
}

import InklingCore
import SwiftUI

/// The newsletter address and its Copy button, while signed in.
struct NewsletterRow: View {
    @Environment(Shelf.self) private var shelf

    var body: some View {
        if let address = shelf.account.newsletterAddress {
            LabeledContent {
                Button(shelf.account.copiedNewsletter ? "Copied" : "Copy") { shelf.account.copyNewsletterAddress() }
                    .accessibilityIdentifier("copy-newsletter-address")
            } label: {
                Text(address)
                    .font(.system(.body, design: .monospaced))
                    .textSelection(.enabled)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
    }
}

/// Highlights and Archive, each with how many it holds.
struct ReadingRows: View {
    @Environment(Shelf.self) private var shelf

    var body: some View {
        NavigationLink {
            HighlightsView()
        } label: {
            LabeledContent("Highlights", value: "\(shelf.allHighlights().reduce(0) { $0 + $1.passages.count })")
        }
        .accessibilityIdentifier("profile-highlights")
        NavigationLink {
            ArchiveView()
        } label: {
            LabeledContent("Archive", value: "\(shelf.archive.count)")
        }
        .accessibilityIdentifier("profile-archive")
    }
}

#if os(iOS)
/// Profile on iPhone: the account and sync, what has been read, where newsletters go, and the
/// settings that were behind the cog.
struct ProfileView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette

    private var account: Account { shelf.account }

    var body: some View {
        Form {
            SwiftUI.Section {
                SettingsCard {
                    if account.signedIn {
                        LabeledContent {
                            if account.syncStopped {
                                Button("Try again") { account.syncNow() }
                                    .accessibilityIdentifier("sync-try-again")
                            } else if account.syncing {
                                ProgressView().controlSize(.small)
                            }
                        } label: {
                            HStack(spacing: 8) {
                                if account.syncStopped {
                                    Circle().fill(Color(palette.ink)).frame(width: 6, height: 6)
                                        .accessibilityIdentifier("sync-dot")
                                }
                                Text(account.syncStopped ? "Sync stopped" : "Signed in with Google")
                            }
                        }
                    } else {
                        Button {
                            account.signIn()
                        } label: {
                            HStack {
                                Text("Sign in with Google")
                                    .font(.inter(16, weight: .semibold, relativeTo: .body))
                                Spacer()
                                if account.signingIn { ProgressView().controlSize(.small) }
                            }
                        }
                        .disabled(account.signingIn)
                        .accessibilityIdentifier("sign-in-google")
                    }
                }
            } footer: {
                if !account.signedIn {
                    palette.caption("Sync your library and drafts, follow feeds and receive newsletters.")
                }
            }

            SwiftUI.Section {
                SettingsCard {
                    ReadingRows()
                }
            }

            if account.signedIn {
                SwiftUI.Section {
                    SettingsCard {
                        NewsletterRow()
                    }
                } header: {
                    palette.caption("Newsletters")
                }
            }

            SwiftUI.Section {
                SettingsCard {
                    NavigationLink {
                        AccountSettingsView(includeFeeds: true)
                            .navigationTitle("Account")
                    } label: {
                        Label("Account, Feeds and Readwise", systemImage: "person.crop.circle")
                    }
                    .accessibilityIdentifier("settings")
                    NavigationLink {
                        ScrollView {
                            AppearancePanel(target: .reading, includeListDensity: true)
                        }
                        .background(Color(palette.ground))
                        .navigationTitle("Appearance")
                    } label: {
                        Label("Appearance", systemImage: "textformat.size")
                    }
                    .accessibilityIdentifier("profile-appearance")
                    NavigationLink {
                        ProviderPanel()
                            .navigationTitle("AI Provider")
                    } label: {
                        Label("AI Provider", systemImage: "sparkles")
                    }
                    .accessibilityIdentifier("profile-provider")
                }
            }
        }
        .settingsForm(palette)
    }
}
#else
/// Profile in the Mac's Settings: where newsletters go, and the highlights and the archive.
struct ProfileSettingsView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette

    var body: some View {
        NavigationStack {
            Form {
                if shelf.account.signedIn {
                    SwiftUI.Section { SettingsCard { NewsletterRow() } } header: {
                        palette.caption("Newsletters")
                    }
                }
                SwiftUI.Section {
                    SettingsCard {
                        ReadingRows()
                    }
                } header: {
                    palette.caption("Reading")
                }
            }
            .settingsForm(palette)
        }
    }
}
#endif

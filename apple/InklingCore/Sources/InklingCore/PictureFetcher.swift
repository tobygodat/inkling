import Foundation

/// Fetches an article's pictures over HTTPS while `Library.saveArticle` runs, which is always off
/// the main thread. A picture that fails, or that takes longer than `timeout`, is left out.
public final class PictureFetcher: Fetcher {
    private let session: URLSession
    private let timeout: TimeInterval

    public init(session: URLSession = .shared, timeout: TimeInterval = 15) {
        self.session = session
        self.timeout = timeout
    }

    public func fetch(url: String) -> Data? {
        guard let address = URL(string: url), address.scheme == "https" || address.scheme == "http" else { return nil }
        let box = Box()
        let done = DispatchSemaphore(value: 0)
        var request = URLRequest(url: address)
        request.timeoutInterval = timeout
        session.dataTask(with: request) { data, response, _ in
            if let status = (response as? HTTPURLResponse)?.statusCode, (200..<300).contains(status) {
                box.data = data
            }
            done.signal()
        }.resume()
        _ = done.wait(timeout: .now() + timeout)
        return box.data
    }

    /// Carries the answer out of URLSession's callback; the semaphore orders the two accesses.
    private final class Box: @unchecked Sendable {
        var data: Data?
    }
}

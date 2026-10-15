import Foundation
import Network

public struct DiscoveredPeer: Identifiable, Equatable {
    public var id: String { name }
    public let name: String
    public let endpoint: NWEndpoint
}

/// Finds Macs running a receiver (`_edgehop._tcp` over Bonjour), so nobody has to type IP addresses.
/// On recent macOS the app needs the Local Network permission and `NSBonjourServices` in its Info.plist.
public final class PeerBrowser {
    public var onChange: (([DiscoveredPeer]) -> Void)?
    private var browser: NWBrowser?
    private let queue = DispatchQueue(label: "edgehop.browser")
    private let ownName: String?

    /// `ownName`: our own advertised name, hidden from the results.
    public init(ownName: String? = nil) { self.ownName = ownName }

    public func start() {
        stop()
        let browser = NWBrowser(for: .bonjour(type: LinkTransport.serviceType, domain: nil), using: .tcp)
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            guard let self else { return }
            let peers: [DiscoveredPeer] = results.compactMap { result in
                guard case let .service(name, _, _, _) = result.endpoint, name != self.ownName else { return nil }
                return DiscoveredPeer(name: name, endpoint: result.endpoint)
            }.sorted { $0.name < $1.name }
            self.onChange?(peers)
        }
        browser.start(queue: queue)
        self.browser = browser
    }

    public func stop() {
        browser?.cancel()
        browser = nil
    }
}

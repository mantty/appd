import CoreLocation
import Foundation

final class AppdGeolocationPlugin: NSObject, AppdPlugin,
  CLLocationManagerDelegate
{
  let id = "geolocation"

  private let manager = CLLocationManager()
  private var current: [AppdPluginReply] = []
  private var watchers: [UUID: AppdPluginReply] = [:]

  override init() {
    super.init()
    manager.delegate = self
    manager.desiredAccuracy = kCLLocationAccuracyBest
  }

  func call(
    method: String,
    arguments: Any,
    reply: @escaping AppdPluginReply
  ) {
    guard method == "getCurrentPosition" else {
      reply(.failure(.notSupported("\(id).\(method) is not supported")))
      return
    }
    current.append(reply)
    start()
  }

  func subscribe(
    method: String,
    arguments: Any,
    reply: @escaping AppdPluginReply
  ) -> (() -> Void) {
    guard method == "watchPosition" else {
      reply(.failure(.notSupported("\(id).\(method) is not supported")))
      return {}
    }
    let subscription = UUID()
    watchers[subscription] = reply
    start()
    return { [weak self] in
      self?.watchers.removeValue(forKey: subscription)
      self?.stopIfIdle()
    }
  }

  func locationManagerDidChangeAuthorization(
    _ manager: CLLocationManager
  ) {
    switch manager.authorizationStatus {
    case .authorizedAlways, .authorizedWhenInUse:
      manager.startUpdatingLocation()
    case .denied, .restricted:
      fail(
        AppdPluginError(
          name: "NotAllowedError",
          message: "Location permission was denied"
        ))
    case .notDetermined:
      break
    @unknown default:
      fail(.notSupported("Location authorization is not supported"))
    }
  }

  func locationManager(
    _ manager: CLLocationManager,
    didUpdateLocations locations: [CLLocation]
  ) {
    guard let location = locations.last else { return }
    let result = Result<Any?, AppdPluginError>.success(position(location))
    let waiting = current
    current.removeAll()
    for reply in waiting {
      reply(result)
    }
    for reply in watchers.values {
      reply(result)
    }
    stopIfIdle()
  }

  func locationManager(
    _ manager: CLLocationManager,
    didFailWithError error: Error
  ) {
    if (error as? CLError)?.code == .locationUnknown { return }
    fail(
      AppdPluginError(
        name: "NotReadableError",
        message: error.localizedDescription
      ))
  }

  private func start() {
    switch manager.authorizationStatus {
    case .authorizedAlways, .authorizedWhenInUse:
      manager.startUpdatingLocation()
    case .notDetermined:
      manager.requestWhenInUseAuthorization()
    case .denied, .restricted:
      fail(
        AppdPluginError(
          name: "NotAllowedError",
          message: "Location permission was denied"
        ))
    @unknown default:
      fail(.notSupported("Location authorization is not supported"))
    }
  }

  private func fail(_ error: AppdPluginError) {
    let result = Result<Any?, AppdPluginError>.failure(error)
    let replies = current + Array(watchers.values)
    current.removeAll()
    for reply in replies {
      reply(result)
    }
    if watchers.isEmpty {
      manager.stopUpdatingLocation()
    }
  }

  private func stopIfIdle() {
    if current.isEmpty && watchers.isEmpty {
      manager.stopUpdatingLocation()
    }
  }

  private func position(_ location: CLLocation) -> [String: Any] {
    let coordinates = location.coordinate
    return [
      "coords": [
        "latitude": coordinates.latitude,
        "longitude": coordinates.longitude,
        "accuracy": location.horizontalAccuracy,
        "altitude": nullable(
          location.verticalAccuracy >= 0,
          location.altitude
        ),
        "altitudeAccuracy": nullable(
          location.verticalAccuracy >= 0,
          location.verticalAccuracy
        ),
        "heading": nullable(location.course >= 0, location.course),
        "speed": nullable(location.speed >= 0, location.speed),
      ],
      "timestamp": location.timestamp.timeIntervalSince1970 * 1000,
    ]
  }

  private func nullable(_ available: Bool, _ value: Double) -> Any {
    available ? value : NSNull()
  }
}

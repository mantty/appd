import type { Position, PositionCallback, PositionErrorCallback } from "../src/index.js";

export function getCurrentPosition(): Promise<Position> {
  return new Promise((resolve, reject) => {
    const geolocation = browserGeolocation();
    geolocation.getCurrentPosition(
      (position) => {
        resolve(copyPosition(position));
      },
      (error) => {
        reject(positionError(error));
      },
    );
  });
}

export function watchPosition(
  next: PositionCallback,
  error: PositionErrorCallback,
): () => void {
  const geolocation = browserGeolocation();
  const id = geolocation.watchPosition(
    (position) => {
      next(copyPosition(position));
    },
    (failure) => {
      error(positionError(failure));
    },
  );
  return () => {
    geolocation.clearWatch(id);
  };
}

function browserGeolocation(): Geolocation {
  const root: { navigator?: { geolocation?: Geolocation } } = globalThis;
  const geolocation = root.navigator?.geolocation;
  if (geolocation) return geolocation;
  throw new DOMException("Geolocation is unavailable", "NotSupportedError");
}

function copyPosition(position: GeolocationPosition): Position {
  const { coords } = position;
  return {
    coords: {
      latitude: coords.latitude,
      longitude: coords.longitude,
      accuracy: coords.accuracy,
      altitude: coords.altitude,
      altitudeAccuracy: coords.altitudeAccuracy,
      heading: coords.heading,
      speed: coords.speed,
    },
    timestamp: position.timestamp,
  };
}

function positionError(error: GeolocationPositionError): DOMException {
  const name = {
    [error.PERMISSION_DENIED]: "NotAllowedError",
    [error.POSITION_UNAVAILABLE]: "NotReadableError",
    [error.TIMEOUT]: "TimeoutError",
  }[error.code];
  return new DOMException(error.message, name ?? "UnknownError");
}

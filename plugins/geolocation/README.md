# @tokamak/geolocation

Geolocation for tokamak applications and the web.

```ts
import { geolocation } from "@tokamak/geolocation";

const position = await geolocation.getCurrentPosition();

const stop = geolocation.watchPosition(
  (next) => console.log(next.coords.latitude, next.coords.longitude),
  console.error,
);
```

The web implementation uses `navigator.geolocation`. Native tokamak builds use
Core Location on Apple platforms, `LocationManager` on Android, and WebView2's
geolocation implementation on Windows. Calling the API on a platform without
geolocation support throws `NotSupportedError`.

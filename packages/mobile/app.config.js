const { version } = require("./package.json");

const IS_DEV = process.env.APP_VARIANT === "development";
const LOGO = IS_DEV ? "./assets/logo-dev" : "./assets/logo";

export default {
  name: IS_DEV ? "Synced Flicks (Dev)" : "Synced Flicks",
  slug: IS_DEV ? "flicksync-dev" : "flicksync",
  version,
  orientation: "default",
  icon: `${LOGO}/icon.png`,
  userInterfaceStyle: "automatic",
  ios: {
    supportsTablet: true,
    bundleIdentifier: "com.oxymoronical.flicksync",
  },
  android: {
    adaptiveIcon: {
      foregroundImage: `${LOGO}/adaptive-icon.png`,
      backgroundColor: "#ffffff",
    },
    blockedPermissions: ["android.permission.RECORD_AUDIO"],
    package: IS_DEV
      ? "com.oxymoronical.flicksync_dev"
      : "com.oxymoronical.flicksync",
  },
  web: {
    favicon: `${LOGO}/favicon.png`,
  },
  plugins: [
    [
      "expo-splash-screen",
      {
        image: `${LOGO}/adaptive-icon.png`,
        backgroundColor: "#ffffff",
        imageWidth: 200,
      },
    ],
    [
      "expo-navigation-bar",
      {
        enforceContrast: true,
        barStyle: "dark",
        visibility: "visible",
      },
    ],
    [
      "expo-video",
      {
        supportsBackgroundPlayback: true,
        supportsPictureInPicture: true,
      },
    ],
  ],
  extra: {
    eas: {
      projectId: "94a5b9ed-95e6-4aed-b4cb-a04f40174d21",
    },
  },
};

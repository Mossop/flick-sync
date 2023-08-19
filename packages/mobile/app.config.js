const { version } = require("./package.json");

const IS_DEV = process.env.APP_VARIANT === "development";
const LOGO = IS_DEV ? "./assets/logo-dev" : "./assets/logo";

export default {
  name: IS_DEV ? "Synced Flicks (Dev)" : "Synced Flicks",
  slug: "flicksync",
  version,
  orientation: "default",
  icon: `${LOGO}/icon.png`,
  userInterfaceStyle: "automatic",
  splash: {
    image: `${LOGO}/splash.png`,
    resizeMode: "contain",
    backgroundColor: "#ffffff",
  },
  assetBundlePatterns: ["**/*"],
  ios: {
    supportsTablet: true,
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
  extra: {
    eas: {
      projectId: "94a5b9ed-95e6-4aed-b4cb-a04f40174d21",
    },
  },
};

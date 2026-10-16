import { createApp } from "vue";
import App from "@/App.vue";
import { DesktopApplication } from "@/desktop/application";
import { DesktopBridge } from "@/desktop/bridge";
import { DesktopSnapshot } from "@/desktop/snapshot";
import { DesktopTransportNative } from "@/desktop/transport_native";
import { DesktopValidation } from "@/desktop/validation";
import "@/theme/tokens.css";

const bridge = new DesktopBridge(new DesktopTransportNative(), new DesktopValidation(), () =>
  crypto.randomUUID(),
);
const application = new DesktopApplication(bridge, new DesktopSnapshot());
createApp(App, { application }).mount("#app");

import type {MessageSchema} from "./locales/zh-CN";

declare module "vue-i18n" {
    export interface DefineLocaleMessage extends MessageSchema {
    }
}

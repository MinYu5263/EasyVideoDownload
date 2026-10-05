import type {ComputedRef, InjectionKey} from 'vue';

export const contentMotionAllowed: InjectionKey<ComputedRef<boolean>> = Symbol('contentMotionAllowed');

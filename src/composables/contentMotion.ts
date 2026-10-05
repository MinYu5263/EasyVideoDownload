export type MotionDirection = 'left' | 'right' | 'up' | 'down';
export type MotionEffect = 'slide' | 'flip';

// The snapshot only paints the departing view. It never receives events or replaces Vue state.
function snapshotContent(content: HTMLElement) {
    const snapshot = content.cloneNode(true) as HTMLElement;
    const sourceNodes = [content, ...content.querySelectorAll<HTMLElement>('*')];
    const copyNodes = [snapshot, ...snapshot.querySelectorAll<HTMLElement>('*')];
    for (let i = 0; i < sourceNodes.length; i++) {
        const source = sourceNodes[i]!, copy = copyNodes[i]!;
        copy.removeAttribute('id');
        copy.removeAttribute('autofocus');
        copy.removeAttribute('form');
        copy.scrollTop = source.scrollTop;
        copy.scrollLeft = source.scrollLeft;
        if (source.tagName === 'INPUT') {
            // A cloned checked radio with the same name can uncheck the live control.
            copy.removeAttribute('name');
            (copy as HTMLInputElement).value = (source as HTMLInputElement).value;
            (copy as HTMLInputElement).checked = (source as HTMLInputElement).checked;
        } else if (source.tagName === 'TEXTAREA') (copy as HTMLTextAreaElement).value = (source as HTMLTextAreaElement).value;
        else if (source.tagName === 'SELECT') (copy as HTMLSelectElement).selectedIndex = (source as HTMLSelectElement).selectedIndex;
    }
    snapshot.setAttribute('aria-hidden', 'true');
    snapshot.inert = true;
    return {snapshot, sourceNodes, copyNodes};
}

export function createContentMotion(
    contentElement: () => HTMLElement | null | undefined,
    reducedMotion: () => boolean,
    onMoving: (moving: boolean) => void = () => {
    },
) {
    let current: {
        content: HTMLElement; snapshot: HTMLElement; animations: Animation[];
        direction: MotionDirection; effect: MotionEffect; inert: boolean;
        width: number; height: number;
        promise: Promise<void>; resolve: () => void;
    } | undefined;

    function cancel() {
        const previous = current;
        if (!previous) return;
        current = undefined;
        for (const animation of previous.animations) {
            void animation.finished.catch(() => {
            });
            animation.cancel();
        }
        previous.snapshot.remove();
        previous.content.inert = previous.inert;
        previous.resolve();
        onMoving(false);
    }

    function capture(direction: MotionDirection, effect: MotionEffect = 'slide') {
        cancel();
        const content = contentElement();
        if (!content?.parentElement || typeof content.animate !== 'function' || reducedMotion()) return;
        const bounds = content.getBoundingClientRect();
        if (!bounds.width || !bounds.height) return;
        // Descendants cancel synchronously before their transient snapshots can be cloned.
        onMoving(true);
        const {snapshot, sourceNodes, copyNodes} = snapshotContent(content);
        Object.assign(snapshot.style, {
            position: 'absolute', top: '0', left: '0', width: `${bounds.width}px`, height: `${bounds.height}px`,
            margin: '0', pointerEvents: 'none', overflow: 'hidden', zIndex: '1',
        });
        content.parentElement.append(snapshot);
        // Scroll offsets can only be restored once the clone has a layout box.
        for (let i = 0; i < sourceNodes.length; i++) {
            copyNodes[i]!.scrollTop = sourceNodes[i]!.scrollTop;
            copyNodes[i]!.scrollLeft = sourceNodes[i]!.scrollLeft;
        }
        let resolve!: () => void;
        const promise = new Promise<void>(done => {
            resolve = done;
        });
        current = {
            content, snapshot, direction, effect, width: bounds.width, height: bounds.height,
            animations: [], inert: content.inert, promise, resolve
        };
        content.inert = true;
    }

    function play() {
        const state = current;
        if (!state) return;
        const negative = state.direction === 'left' || state.direction === 'up';
        const sign = negative ? -1 : 1;
        const axis = state.direction === 'left' || state.direction === 'right' ? 'X' : 'Y';
        const flip = state.effect === 'flip';
        const distance = flip ? 90 : axis === 'X' ? state.width : state.height;
        const transform = (amount: number) => flip ? `perspective(1000px) rotateY(${amount}deg)` : `translate${axis}(${amount}px)`;
        const duration = flip ? 175 : 300;
        const options: KeyframeAnimationOptions = {duration, easing: 'cubic-bezier(.2,.7,.2,1)', fill: 'both'};
        try {
            state.animations.push(state.snapshot.animate([
                {transform: transform(0)}, {transform: transform(sign * distance)},
            ], options));
            state.animations.push(state.content.animate([
                {transform: transform(-sign * distance)}, {transform: transform(0)},
            ], {...options, delay: flip ? duration : 0}));
            // Both pages must share a timeline so their adjoining edges stay together.
            const startTime = state.content.ownerDocument?.timeline?.currentTime;
            if (startTime != null) for (const animation of state.animations) animation.startTime = startTime;
            void Promise.allSettled(state.animations.map(animation => animation.finished)).then(() => {
                if (current === state) cancel();
            });
        } catch {
            // Animation availability must never prevent navigation or retain an inert view.
            cancel();
        }
    }

    async function whenIdle() {
        while (current) await current.promise;
    }

    return {capture, play, cancel, whenIdle};
}

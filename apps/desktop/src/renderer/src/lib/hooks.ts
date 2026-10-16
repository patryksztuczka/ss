import { Effect, Exit, Fiber } from "effect";
import { useEffect, useRef, useState, type DependencyList } from "react";

import type { EventChannel, EventPayload } from "../../../shared/ipc";
import { subscribe } from "./ipc";

export type AsyncState<A, E> =
  | { readonly _tag: "Loading" }
  | { readonly _tag: "Success"; readonly value: A }
  | { readonly _tag: "Failure"; readonly error: E };

/** Fire-and-forget an Effect from an event handler, logging any failure. */
export const run = <A, E>(effect: Effect.Effect<A, E>) =>
  Effect.runFork(effect.pipe(Effect.tapCause((cause) => Effect.logError(cause))));

/** Runs an Effect when deps change and interrupts it on unmount / re-run. */
export function useEffectState<A, E>(
  make: () => Effect.Effect<A, E>,
  deps: DependencyList,
): AsyncState<A, E> {
  const [state, setState] = useState<AsyncState<A, E>>({ _tag: "Loading" });
  useEffect(() => {
    setState({ _tag: "Loading" });
    const fiber = Effect.runFork(make());
    fiber.addObserver((exit: Exit.Exit<A, E>) => {
      if (Exit.isSuccess(exit)) return setState({ _tag: "Success", value: exit.value });
      const error = Exit.findErrorOption(exit);
      if (error._tag === "Some") setState({ _tag: "Failure", error: error.value });
    });
    return () => void Effect.runFork(Fiber.interrupt(fiber));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return state;
}

export function useEvent<C extends EventChannel>(
  channel: C,
  listener: (payload: EventPayload<C>) => void,
) {
  const latest = useRef(listener);
  latest.current = listener;
  useEffect(() => subscribe(channel, (payload) => latest.current(payload)), [channel]);
}

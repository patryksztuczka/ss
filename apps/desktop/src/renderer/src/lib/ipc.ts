import { Effect, Schema } from "effect";

import {
  Events,
  Invoke,
  IpcResult,
  type EventChannel,
  type EventPayload,
  type InvokeChannel,
  type InvokeInput,
  type InvokeOutput,
} from "../../../shared/ipc";

export class IpcError extends Schema.TaggedError<IpcError>()("IpcError", {
  channel: Schema.String,
  message: Schema.String,
}) {}

const decodeResult = Schema.decodeUnknownEffect(IpcResult);

/** Typed request to the main process; input and output are validated against the shared contract. */
export const invoke = <C extends InvokeChannel>(
  channel: C,
  ...[input]: InvokeInput<C> extends void ? [] : [InvokeInput<C>]
): Effect.Effect<InvokeOutput<C>, IpcError> => {
  const contract = Invoke[channel];
  const fail = (message: string) => new IpcError({ channel, message });
  return Schema.encodeUnknownEffect(contract.input as Schema.Codec<InvokeInput<C>, unknown>)(
    input,
  ).pipe(
    Effect.flatMap((payload) => Effect.tryPromise(() => window.ss.invoke(channel, payload))),
    Effect.flatMap(decodeResult),
    Effect.mapError((error) => fail(error.message)),
    Effect.flatMap((result) =>
      result.ok
        ? Schema.decodeUnknownEffect(contract.output as Schema.Codec<InvokeOutput<C>, unknown>)(
            result.value,
          ).pipe(Effect.mapError((error) => fail(error.message)))
        : Effect.fail(fail(result.error)),
    ),
    Effect.withSpan(`ipc.${channel}`),
  );
};

/** Subscribes to a main → renderer event. Malformed payloads are logged and dropped. */
export const subscribe = <C extends EventChannel>(
  channel: C,
  listener: (payload: EventPayload<C>) => void,
): (() => void) => {
  const decode = Schema.decodeUnknownEffect(
    Events[channel] as Schema.Codec<EventPayload<C>, unknown>,
  );
  return window.ss.on(channel, (raw) => {
    Effect.runFork(
      decode(raw).pipe(
        Effect.flatMap((payload) => Effect.sync(() => listener(payload))),
        Effect.catch((error) => Effect.logWarning(`Dropped ${channel} event`, error)),
      ),
    );
  });
};

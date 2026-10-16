import { Schema } from "effect";

export class CaptureError extends Schema.TaggedError<CaptureError>()("CaptureError", {
  message: Schema.String,
  cause: Schema.optionalKey(Schema.Defect()),
}) {}

/** macOS refused to give us pixels — almost always a missing Screen Recording permission. */
export class ScreenPermissionError extends Schema.TaggedError<ScreenPermissionError>()(
  "ScreenPermissionError",
  { detail: Schema.String },
) {}

export class RecorderError extends Schema.TaggedError<RecorderError>()("RecorderError", {
  message: Schema.String,
  cause: Schema.optionalKey(Schema.Defect()),
}) {}

export class OcrError extends Schema.TaggedError<OcrError>()("OcrError", {
  message: Schema.String,
  cause: Schema.optionalKey(Schema.Defect()),
}) {}

export class StorageError extends Schema.TaggedError<StorageError>()("StorageError", {
  message: Schema.String,
  cause: Schema.optionalKey(Schema.Defect()),
}) {}

export class CaptureNotFound extends Schema.TaggedError<CaptureNotFound>()("CaptureNotFound", {
  id: Schema.String,
}) {}

export class HelperError extends Schema.TaggedError<HelperError>()("HelperError", {
  message: Schema.String,
  cause: Schema.optionalKey(Schema.Defect()),
}) {}

import { createContext, type ReactNode, useContext } from "react";

export type RecordingDestination = {
  spaceId: string;
  spaceName: string;
  parentNodeId: string;
  destinationPath: string;
};

export type RecordingStatus = "idle" | "requesting" | "recording" | "stopping";

export type RecordingState = {
  status: RecordingStatus;
  startedAt: number | null;
  filename: string | null;
  destinationPath: string | null;
};

export type RecordingActions = {
  startRecording: (destination: RecordingDestination) => Promise<void>;
  stopRecording: () => void;
  discardRecording: () => void;
};

const SIGNAL_BAR_COUNT = 12;
const EMPTY_SIGNAL = Array.from({ length: SIGNAL_BAR_COUNT }, () => 0);
export const IDLE_RECORDING_STATE: RecordingState = {
  status: "idle",
  startedAt: null,
  filename: null,
  destinationPath: null
};
const RecordingStateContext = createContext<RecordingState | null>(null);
const RecordingSignalContext = createContext<number[]>(EMPTY_SIGNAL);
const RecordingActionsContext = createContext<RecordingActions | null>(null);

export function AudioRecordingContextProvider({
  actions,
  children,
  signal,
  state
}: {
  actions: RecordingActions;
  children: ReactNode;
  signal: number[];
  state: RecordingState;
}) {
  return (
    <RecordingActionsContext.Provider value={actions}>
      <RecordingStateContext.Provider value={state}>
        <RecordingSignalContext.Provider value={signal}>{children}</RecordingSignalContext.Provider>
      </RecordingStateContext.Provider>
    </RecordingActionsContext.Provider>
  );
}

export function useAudioRecordingState(): RecordingState {
  const state = useContext(RecordingStateContext);
  if (!state) throw new Error("AudioRecordingProvider is missing");
  return state;
}

export function useAudioRecordingSignal(): number[] {
  return useContext(RecordingSignalContext);
}

export function useAudioRecordingActions(): RecordingActions {
  const actions = useContext(RecordingActionsContext);
  if (!actions) throw new Error("AudioRecordingProvider is missing");
  return actions;
}

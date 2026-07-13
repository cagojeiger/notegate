import { useEffect, useRef, useState } from "react";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

type TextContent = ReadTextResponse["text"];
type EditorMode = "preview" | "edit";

export function useTextEditorSession({
  node,
  latestNode,
  mode,
  canWrite,
  onSetMode
}: {
  node: RestNode;
  latestNode?: RestNode;
  mode: EditorMode;
  canWrite: boolean;
  onSetMode: (mode: EditorMode) => void;
}) {
  const textQuery = useTextDocument(node);
  const [draft, setDraft] = useState("");
  const [conflict, setConflict] = useState(false);
  const [externalUpdate, setExternalUpdate] = useState<RestNode | null>(null);
  const previousMode = useRef<EditorMode>(mode);
  const draftNodeId = useRef<string | null>(null);
  const lastAutoReloadSha = useRef<string | null>(null);
  const dismissedExternalSha = useRef<string | null>(null);
  const activeNodeId = useRef(node.id);
  activeNodeId.current = node.id;
  const showToast = useUiStore((state) => state.showToast);

  const text = textQuery.data?.text;
  const plainText = isPlainTextContent(text) ? text : null;
  const content = plainText?.content ?? "";
  const sha = text?.content_sha256;
  const encrypted = isEncryptedTextContent(text);
  const partialText = plainText?.truncated ? plainText : null;
  const canEdit = canWrite && !!plainText && !partialText;
  const canCopy = !!plainText && !partialText;
  const dirty = mode === "edit" && canEdit && draft !== content;

  useEffect(() => {
    setConflict(false);
    setExternalUpdate(null);
    lastAutoReloadSha.current = null;
    dismissedExternalSha.current = null;
  }, [node.id]);

  useEffect(() => {
    if (mode === "edit" && canEdit && (previousMode.current !== "edit" || draftNodeId.current !== node.id)) {
      setDraft(content);
      draftNodeId.current = node.id;
    }
    previousMode.current = mode;
  }, [mode, content, canEdit, node.id]);

  useEffect(() => {
    if (mode === "edit" && textQuery.isSuccess && !canEdit) onSetMode("preview");
  }, [canEdit, mode, onSetMode, textQuery.isSuccess]);

  async function reloadLatestText(showFailure: boolean) {
    const requestedNodeId = node.id;
    const result = await textQuery.refetch();
    if (activeNodeId.current !== requestedNodeId) return false;

    const nextText = result.data?.text;
    if (result.isError || !nextText) {
      if (showFailure) showToast("Could not reload latest text");
      return false;
    }

    if (isPlainTextContent(nextText)) setDraft(nextText.content);
    setExternalUpdate(null);
    dismissedExternalSha.current = null;
    return true;
  }

  useEffect(() => {
    const latestSha = latestNode?.content_sha256;
    if (!latestNode || !latestSha || !sha) return;

    if (latestSha === sha) {
      if (externalUpdate?.content_sha256 === latestSha) setExternalUpdate(null);
      lastAutoReloadSha.current = null;
      return;
    }

    if (dirty) {
      if (dismissedExternalSha.current !== latestSha) setExternalUpdate(latestNode);
      return;
    }

    if (lastAutoReloadSha.current === latestSha) return;
    lastAutoReloadSha.current = latestSha;
    void reloadLatestText(false).then((reloaded) => {
      if (!reloaded && activeNodeId.current === node.id && lastAutoReloadSha.current === latestSha) {
        lastAutoReloadSha.current = null;
      }
    });
  }, [dirty, externalUpdate?.content_sha256, latestNode, sha]);

  const saveMutation = useSaveTextDocument(
    node,
    draft,
    sha,
    () => {
      setConflict(false);
      onSetMode("preview");
    },
    () => setConflict(true)
  );

  function cancelEdit() {
    if (dirty) {
      setDraft(content);
      showToast("Edit canceled");
    }
    setConflict(false);
    setExternalUpdate(null);
    onSetMode("preview");
  }

  return {
    textQuery,
    content,
    draft,
    setDraft,
    encrypted,
    partialText,
    canEdit,
    canCopy,
    dirty,
    conflict,
    externalUpdate,
    canSave: dirty && !saveMutation.isPending,
    saveDraft: () => saveMutation.mutate(false),
    overwriteDraft: () => saveMutation.mutate(true),
    cancelEdit,
    reloadConflict: () => {
      void reloadLatestText(true).then((reloaded) => {
        if (reloaded) setConflict(false);
      });
    },
    reloadExternalUpdate: () => {
      void reloadLatestText(true).then((reloaded) => {
        if (reloaded) onSetMode("preview");
      });
    },
    dismissExternalUpdate: () => {
      dismissedExternalSha.current = externalUpdate?.content_sha256 ?? null;
      setExternalUpdate(null);
    }
  };
}

function isPlainTextContent(text: TextContent | undefined): text is Extract<TextContent, { storage_format: "plain" }> {
  return !!text && text.storage_format === "plain" && "content" in text;
}

function isEncryptedTextContent(text: TextContent | undefined): text is Extract<TextContent, { storage_format: "encrypted" }> {
  return !!text && text.storage_format === "encrypted" && "encrypted_payload" in text;
}

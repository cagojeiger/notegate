import { useCallback, useEffect, useRef, useState } from "react";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { canWriteNode } from "../nodes/nodeWriteAccess";
import { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

type TextContent = ReadTextResponse["text"];
type EditorMode = "preview" | "edit";
type EditBaseline = { nodeId: string; content: string; sha: string };

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
  const [baseline, setBaseline] = useState<EditBaseline | null>(null);
  const [editSessionId, setEditSessionId] = useState(() => crypto.randomUUID());
  const [conflict, setConflict] = useState(false);
  const [externalUpdate, setExternalUpdate] = useState<RestNode | null>(null);
  const previousMode = useRef<EditorMode>(mode);
  const lastAutoReloadSha = useRef<string | null>(null);
  const dismissedExternalSha = useRef<string | null>(null);
  const mounted = useRef(true);
  const activeEditor = useRef({ nodeId: node.id, editSessionId, draft, mode });
  activeEditor.current = { nodeId: node.id, editSessionId, draft, mode };
  const showToast = useUiStore((state) => state.showToast);

  const text = textQuery.data?.text;
  const refetchText = textQuery.refetch;
  const plainText = isPlainTextContent(text) ? text : null;
  const content = plainText?.content ?? "";
  const sha = text?.content_sha256;
  const encrypted = isEncryptedTextContent(text);
  const partialText = plainText?.truncated ? plainText : null;
  const canEdit = canWriteNode(node, canWrite) && !!plainText && !partialText;
  const canCopy = !!plainText && !partialText;
  const editBaseline = baseline?.nodeId === node.id ? baseline : null;
  const dirty = mode === "edit" && !!editBaseline && draft !== editBaseline.content;
  const comparisonSha = mode === "edit" ? editBaseline?.sha ?? sha : sha;

  const saveMutation = useSaveTextDocument(
    ({ request, response }) => {
      const active = activeEditor.current;
      if (!mounted.current || active.nodeId !== request.node.id || active.editSessionId !== request.editSessionId || active.mode !== "edit") return;
      setBaseline({ nodeId: request.node.id, content: request.content, sha: response.text.content_sha256 });
      setConflict(false);
      setExternalUpdate(null);
      dismissedExternalSha.current = null;
      setEditSessionId(crypto.randomUUID());
      if (active.draft === request.content) {
        onSetMode("preview");
      }
    },
    (request) => {
      const active = activeEditor.current;
      if (mounted.current && active.nodeId === request.node.id && active.editSessionId === request.editSessionId && active.mode === "edit") setConflict(true);
    }
  );

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    setEditSessionId(crypto.randomUUID());
    setConflict(false);
    setExternalUpdate(null);
    lastAutoReloadSha.current = null;
    dismissedExternalSha.current = null;
  }, [node.id]);

  useEffect(() => {
    if (mode === "edit" && canEdit && sha && (
      previousMode.current !== "edit" || !editBaseline
      || (!dirty && !saveMutation.isPending && editBaseline.sha !== sha)
    )) {
      setDraft(content);
      setBaseline({ nodeId: node.id, content, sha });
    }
    previousMode.current = mode;
  }, [mode, content, sha, canEdit, node.id, editBaseline, dirty, saveMutation.isPending]);

  useEffect(() => {
    const preserveLockedDraft = node.effective_write_locked && dirty;
    if (mode === "edit" && textQuery.isSuccess && !canEdit && !preserveLockedDraft) {
      onSetMode("preview");
    }
  }, [canEdit, dirty, mode, node.effective_write_locked, onSetMode, textQuery.isSuccess]);

  const reloadLatestText = useCallback(async (showFailure: boolean) => {
    const requested = activeEditor.current;
    const result = await refetchText();
    const active = activeEditor.current;
    if (!mounted.current || active.nodeId !== requested.nodeId || active.editSessionId !== requested.editSessionId || active.mode !== requested.mode || active.draft !== requested.draft) return false;

    const nextText = result.data?.text;
    if (result.isError || !nextText) {
      if (showFailure) showToast("Could not reload latest text");
      return false;
    }

    if (isPlainTextContent(nextText)) {
      setDraft(nextText.content);
      setBaseline({ nodeId: requested.nodeId, content: nextText.content, sha: nextText.content_sha256 });
    }
    setExternalUpdate(null);
    dismissedExternalSha.current = null;
    return true;
  }, [refetchText, showToast]);

  useEffect(() => {
    // Body refreshes (including saves in another pane) must not advance a dirty draft's baseline.
    const observedNode = sha && sha !== comparisonSha ? { ...node, content_sha256: sha } : latestNode;
    const latestSha = observedNode?.content_sha256;
    if (!observedNode || !latestSha || !comparisonSha) return;

    if (latestSha === comparisonSha) {
      if (externalUpdate?.content_sha256) setExternalUpdate(null);
      lastAutoReloadSha.current = null;
      return;
    }

    if (dirty || saveMutation.isPending) {
      if (dismissedExternalSha.current !== latestSha) setExternalUpdate(observedNode);
      return;
    }

    if (sha === latestSha) return;
    if (lastAutoReloadSha.current === latestSha) return;
    lastAutoReloadSha.current = latestSha;
    void reloadLatestText(false).then((reloaded) => {
      if (!reloaded && activeEditor.current.nodeId === node.id && lastAutoReloadSha.current === latestSha) {
        lastAutoReloadSha.current = null;
      }
    });
  }, [dirty, externalUpdate?.content_sha256, latestNode, node, reloadLatestText, sha, comparisonSha, saveMutation.isPending]);

  function saveDraft(force: boolean) {
    if (!canEdit || !editBaseline || saveMutation.isPending) return;
    saveMutation.mutate({ node, content: draft, expectedSha256: editBaseline.sha, editSessionId, force });
  }

  function cancelEdit() {
    setEditSessionId(crypto.randomUUID());
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
    saving: saveMutation.isPending,
    resetRevisionSession: () => setEditSessionId(crypto.randomUUID()),
    canSave: canEdit && dirty && !saveMutation.isPending,
    saveDraft: () => saveDraft(false),
    overwriteDraft: () => saveDraft(true),
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

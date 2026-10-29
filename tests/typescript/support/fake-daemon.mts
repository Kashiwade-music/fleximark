import { PROTOCOL_VERSION } from "@fleximark/contracts/protocol";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { request } from "node:http";
import { join } from "node:path";

export interface DaemonInstance {
  pid: number;
  daemonInstanceId: string;
  url: string;
}

export interface RecordedMessage {
  pid: number;
  direction: "received" | "sent";
  message: {
    jsonrpc: string;
    id?: number | string;
    method?: string;
    params?: Record<string, unknown>;
    result?: unknown;
  };
}

export interface DaemonSnapshot {
  documents: {
    uri: string;
    version: number;
    text: string;
    sessionId: string;
  }[];
  previews: {
    id: string;
    target: string;
    frame: import("@fleximark/contracts/protocol").RenderFrame;
  }[];
}

/** Runs a real child process and exchanges only framed protocol messages. */
export async function createFakeDaemon(root: string) {
  const instancesFile = join(root, "fake-daemon.instances.jsonl");
  const messagesFile = join(root, "fake-daemon.messages.jsonl");
  await mkdir(root, { recursive: true });
  await writeFile(instancesFile, "");
  await writeFile(messagesFile, "");
  const script = `const root = ${JSON.stringify(root)};
const protocolVersion = ${PROTOCOL_VERSION};
${daemonScript}`;
  const install = async (directory: string) => {
    await mkdir(directory, { recursive: true });
    await writeFile(join(directory, "lsp"), script);
  };
  await install(root);
  const lines = async <T,>(file: string): Promise<T[]> =>
    (await readFile(file, "utf8"))
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line) as T);
  const instances = () => lines<DaemonInstance>(instancesFile);
  const control = async (action: string, value: unknown = {}, pid?: number) => {
    const instance = (await instances()).findLast(
      (entry) => pid === undefined || entry.pid === pid,
    );
    if (!instance) throw new Error("The fake daemon has not started");
    return await new Promise<unknown>((resolve, reject) => {
      const call = request(
        instance.url,
        { method: "POST", headers: { "Content-Type": "application/json" } },
        (response) => {
          let body = "";
          response.setEncoding("utf8");
          response.on("data", (chunk: string) => {
            body += chunk;
          });
          response.on("error", reject);
          response.on("end", () => {
            try {
              if (response.statusCode !== 200) throw new Error(body);
              resolve(JSON.parse(body) as unknown);
            } catch (error) {
              reject(error);
            }
          });
        },
      );
      call.on("error", reject);
      call.end(JSON.stringify({ action, value }));
    });
  };
  return {
    install,
    instances,
    messages: () => lines<RecordedMessage>(messagesFile),
    send: (message: unknown, pid?: number) => control("send", message, pid),
    hold: (method: string, pid?: number) => control("hold", method, pid),
    release: (pid?: number) => control("release", {}, pid),
    crash: (pid?: number) => control("crash", {}, pid),
    snapshot: async (pid?: number) =>
      (await control("snapshot", {}, pid)) as DaemonSnapshot,
  };
}

// Keep the child independent of compiled test bundle paths and module loaders.
const daemonScript = String.raw`
const fs = require("node:fs");
const path = require("node:path");
const http = require("node:http");
const daemonInstanceId = "daemon-" + process.pid;
const documents = new Map();
const previews = new Map();
const heldMethods = new Set();
const heldResponses = [];
let sequence = 0;
let url;
let input = Buffer.alloc(0);
const log = (direction, message) => fs.appendFileSync(path.join(root, "fake-daemon.messages.jsonl"), JSON.stringify({pid: process.pid, direction, message}) + "\n");
const send = (message) => {
  log("sent", message);
  const body = Buffer.from(JSON.stringify(message));
  // Split headers and body so the adapter must use its real framing parser.
  process.stdout.write("Content-Length: " + body.length + "\r\n\r\n");
  process.stdout.write(body);
};
const reply = (request, result) => {
  const response = {jsonrpc:"2.0", id:request.id, result};
  if (heldMethods.has(request.method)) heldResponses.push(response);
  else send(response);
};
const range = () => ({byteStart:0,byteEnd:1,start:{line:0,character:0,encoding:"utf16"},end:{line:0,character:1,encoding:"utf16"}});
const escape = (text) => text.replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;");
const frame = (preview, revision) => ({
  previewSessionId:preview.id,documentVersion:preview.document.version,renderRevision:revision,
  rendererFingerprint:"a".repeat(64),style:null,assets:[],
  blocks:[{id:"node",nodeIds:["node"],html:'<p data-fleximark-node-id="node">'+escape(preview.document.text)+"</p>"}],
  navigation:[{nodeId:"node",depth:0,sourceRange:range()}]
});
const publish = (preview) => {
  preview.frame = frame(preview, preview.frame.renderRevision + 1);
  send({jsonrpc:"2.0",method:"fleximark/previewChanged",params:{daemonInstanceId,previewSessionId:preview.id,renderRevision:preview.frame.renderRevision,frame:preview.frame}});
};
const handle = (message) => {
  log("received", message);
  const p = message.params || {};
  if (message.method === "initialize") {
    const workspaces = p.workspaceFolders || [];
    reply(message,{capabilities:{experimental:{fleximark:{protocolVersion,daemonInstanceId,capabilities:{htmlRender:true,selectionEvents:true,viewportEvents:true,workspaceCommands:[]},workspaceStatuses:workspaces.map(w=>({uri:w.uri,enabled:true}))}}}});
  } else if (message.method === "textDocument/didOpen") {
    const d=p.textDocument; documents.set(d.uri,{uri:d.uri,version:d.version,text:d.text,sessionId:daemonInstanceId+"/document/"+(++sequence)});
  } else if (message.method === "textDocument/didChange") {
    const d=documents.get(p.textDocument.uri); if(d){ d.version=p.textDocument.version; d.text=p.contentChanges[0].text; for(const preview of previews.values()) if(preview.document===d) publish(preview); }
  } else if (message.method === "textDocument/didClose") {
    documents.delete(p.textDocument.uri);
  } else if (message.method === "fleximark/attachDocument") {
    const d=documents.get(p.uri); reply(message,{documentSessionId:d.sessionId,documentVersion:d.version});
  } else if (message.method === "fleximark/createPreview") {
    const d=[...documents.values()].find(d=>d.sessionId===p.documentSessionId);
    const preview={id:daemonInstanceId+"/preview/"+(++sequence),document:d,target:p.target};
    preview.frame=frame(preview,1); previews.set(preview.id,preview);
    reply(message,{previewSessionId:preview.id,frame:preview.frame,...(p.target==="externalBrowser"?{url:url+"/preview/"+encodeURIComponent(preview.id)}:{})});
  } else if (message.method === "fleximark/rerenderPreview") {
    const preview=previews.get(p.previewSessionId); if(preview) publish(preview); reply(message,null);
  } else if (message.method === "fleximark/disposePreview") {
    previews.delete(p.previewSessionId); reply(message,null);
  } else if (message.method === "fleximark/reconfigureWorkspace") {
    reply(message,null);
  } else if (message.method === "textDocument/hover") {
    reply(message,{contents:"hover from "+daemonInstanceId});
  } else if (message.method === "shutdown") reply(message,null);
  else if (message.method === "exit") process.exit(0);
  else if (message.id !== undefined) reply(message,null);
};
process.stdin.on("data",chunk=>{
  input=Buffer.concat([input,chunk]);
  for(;;){const end=input.indexOf("\r\n\r\n");if(end<0)return;const length=Number(/Content-Length: (\d+)/i.exec(input.subarray(0,end).toString())[1]);if(input.length<end+4+length)return;const body=input.subarray(end+4,end+4+length);input=input.subarray(end+4+length);handle(JSON.parse(body.toString()));}
});
process.stdin.on("end",()=>process.exit(0));
const server=http.createServer((request,response)=>{
  let body=""; request.on("data",chunk=>body+=chunk); request.on("end",()=>{
    try {
      const {action,value}=JSON.parse(body);
      let result={};
      if(action==="send")send(value);
      if(action==="hold")heldMethods.add(value);
      if(action==="release"){heldMethods.clear();for(const item of heldResponses.splice(0))send(item);}
      if(action==="snapshot")result={documents:[...documents.values()],previews:[...previews.values()].map(({id,target,frame})=>({id,target,frame}))};
      response.setHeader("Content-Type","application/json");response.end(JSON.stringify(result));
      if(action==="crash")setImmediate(()=>process.exit(17));
    } catch(error){response.statusCode=500;response.end(String(error));}
  });
});
server.listen(0,"127.0.0.1",()=>{
  url="http://127.0.0.1:"+server.address().port;
  fs.appendFileSync(path.join(root,"fake-daemon.instances.jsonl"),JSON.stringify({pid:process.pid,daemonInstanceId,url})+"\n");
});
`;

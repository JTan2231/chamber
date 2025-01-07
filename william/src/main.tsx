import { useState, useEffect, useCallback, useRef } from 'react';
import React from 'react';
import ReactDOM from 'react-dom/client';
import MarkdownIt from 'markdown-it';
import markdownItKatex from 'markdown-it-katex';
import { z } from 'zod';
import hljs from 'highlight.js';

import './font.css';
import './buttons.css';

const md = new MarkdownIt({
  html: true,
  linkify: true,
  typographer: true,
  highlight: function (str, lang) {
    if (lang && hljs.getLanguage(lang)) {
      try {
        return hljs.highlight(str, { language: lang }).value;
      } catch (__) { }
    }
    return ''; // use external default escaping
  }
}).use(markdownItKatex);

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);

interface WebSocketHookOptions {
  url: string;
  retryInterval?: number;
  maxRetries?: number;
}

interface WebSocketHookReturn {
  socket: WebSocket | null;
  systemPrompt: string;
  conversations: Conversation[];
  loadedConversation: Conversation;
  setLoadedConversation: Function;
  sendMessage: (message: ArrakisRequest) => void;
  connectionStatus: 'connecting' | 'connected' | 'disconnected';
  error: Error | null;
}

const OpenAIModelSchema = z.enum([
  "gpt-4o",
  "gpt-4o-mini",
  "o1-preview",
  "o1-mini",
]);

const GroqModelSchema = z.enum([
  "llama3-70b-8192",
]);

const AnthropicModelSchema = z.enum([
  "claude-3-opus-20240229",
  "claude-3-sonnet-20240229",
  "claude-3-haiku-20240307",
  "claude-3-5-sonnet-latest",
  "claude-3-5-haiku-latest",
]);

const APISchema = z.discriminatedUnion("provider", [
  z.object({
    provider: z.literal("openai"),
    model: OpenAIModelSchema,
  }),
  z.object({
    provider: z.literal("groq"),
    model: GroqModelSchema,
  }),
  z.object({
    provider: z.literal("anthropic"),
    model: AnthropicModelSchema,
  }),
]);

const MessageSchema = z.object({
  message_type: z.enum(["System", "User", "Assistant"]),
  id: z.number().nullable(),
  content: z.string(),
  api: APISchema,
  system_prompt: z.string(),
  sequence: z.number(),
});

const ConversationSchema = z.object({
  id: z.number().nullable(),
  name: z.string(),
  messages: z.array(MessageSchema),
});

const CompletionRequestSchema = ConversationSchema;

const SystemPromptRequestSchema = z.object({
  content: z.string(),
  write: z.boolean(),
});

const PingRequestSchema = z.object({
  body: z.string(),
});

const LoadRequestSchema = z.object({
  id: z.number(),
});

const ForkRequestSchema = z.object({
  conversationId: z.number(),
  sequence: z.number(),
});

const CompletionResponseSchema = z.object({
  stream: z.boolean(),
  delta: z.string(),
  name: z.string(),
  conversationId: z.number(),
  requestId: z.number(),
  responseId: z.number(),
});

const SystemPromptResponseSchema = SystemPromptRequestSchema;

const PingResponseSchema = PingRequestSchema;

const ConversationListResponseSchema = z.object({
  conversations: z.array(ConversationSchema),
});

const ArrakisRequestSchema = z.discriminatedUnion("method", [
  z.object({
    method: z.literal("ConversationList"),
  }),
  z.object({
    method: z.literal("Ping"),
    payload: PingRequestSchema,
  }),
  z.object({
    method: z.literal("Completion"),
    payload: CompletionRequestSchema,
  }),
  z.object({
    method: z.literal("Load"),
    payload: LoadRequestSchema,
  }),
  z.object({
    method: z.literal("SystemPrompt"),
    payload: SystemPromptRequestSchema,
  }),
  z.object({
    method: z.literal("Fork"),
    payload: ForkRequestSchema,
  }),
]);

const ArrakisResponseSchema = z.discriminatedUnion("method", [
  z.object({
    method: z.literal("ConversationList"),
    payload: ConversationListResponseSchema,
  }),
  z.object({
    method: z.literal("Ping"),
    payload: PingResponseSchema,
  }),
  z.object({
    method: z.literal("Completion"),
    payload: CompletionResponseSchema,
  }),
  z.object({
    method: z.literal("SystemPrompt"),
    payload: SystemPromptResponseSchema,
  }),
]);

type API = z.infer<typeof APISchema>;
type Message = z.infer<typeof MessageSchema>;
type Conversation = z.infer<typeof ConversationSchema>;
type SystemPromptRequest = z.infer<typeof SystemPromptRequestSchema>;
type PingRequest = z.infer<typeof PingRequestSchema>;
type LoadRequest = z.infer<typeof LoadRequestSchema>;
type ArrakisRequest = z.infer<typeof ArrakisRequestSchema>;
type ArrakisResponse = z.infer<typeof ArrakisResponseSchema>;

// TODO: disgusting mixing of concerns between this and the main page
//       should probably centralize everything dealing with message responses
//       in here + separate away from rendering

interface TitleCaseOptions {
  preserveAcronyms?: boolean;
  handleHyphens?: boolean;
  customMinorWords?: string[];
}

function formatTitle(input: string, options: TitleCaseOptions = {}): string {
  if (!input) return '';

  const {
    preserveAcronyms = true,
    handleHyphens = true,
    customMinorWords = [],
  } = options;

  const MINOR_WORDS = new Set([
    'a', 'an', 'the', 'and', 'but', 'or', 'nor', 'for', 'yet', 'so',
    'in', 'on', 'at', 'to', 'for', 'of', 'with', 'by',
    ...customMinorWords
  ]);

  const isAcronym = (word: string): boolean => {
    return /^[A-Z0-9]+$/.test(word);
  };

  const capitalizeWord = (word: string, forceCapitalize: boolean = false): string => {
    if (preserveAcronyms && isAcronym(word)) {
      return word;
    }

    const wordLower = word.toLowerCase();

    if (forceCapitalize || !MINOR_WORDS.has(wordLower)) {
      return word.charAt(0).toUpperCase() + wordLower.slice(1);
    }

    return wordLower;
  };

  const processHyphenatedWord = (word: string, forceCapitalize: boolean): string => {
    if (!handleHyphens) return capitalizeWord(word, forceCapitalize);

    return word = word
      .split('-')
      .map((part, index) => capitalizeWord(part, index === 0 && forceCapitalize))
      .join('-');
  };

  const words = input.split(/\s+/);

  const capitalizedWords = words.map((word, index) => {
    const isFirst = index === 0;
    const isLast = index === words.length - 1;

    return processHyphenatedWord(word, isFirst || isLast);
  });

  return capitalizedWords.join(' ').replace('.json', '').replaceAll('_', ' ');
}

function conversationDefault(): Conversation {
  return ConversationSchema.parse({ id: null, name: crypto.randomUUID(), messages: [] });
}

const useWebSocket = ({
  url,
  retryInterval = 5000,
  maxRetries = 0
}: WebSocketHookOptions): WebSocketHookReturn => {
  const [socket, setSocket] = useState<WebSocket | null>(null);
  const [systemPrompt, setSystemPrompt] = useState<string>('');
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [loadedConversation, setLoadedConversation] = useState<Conversation>(conversationDefault());
  const [connectionStatus, setConnectionStatus] = useState<'connected' | 'disconnected'>('disconnected');
  const [error, setError] = useState<Error | null>(null);
  const [retryCount, setRetryCount] = useState(0);

  const connect = useCallback(() => {
    try {
      const ws = new WebSocket(url);

      ws.onopen = () => {
        setConnectionStatus('connected');
        setError(null);
        setRetryCount(0);
      };

      ws.onmessage = (event) => {
        try {
          const response = JSON.parse(event.data) satisfies ArrakisResponse;
          if (response.payload.method === 'Completion') {
            setLoadedConversation(prev => {
              const completion = CompletionResponseSchema.parse(response.payload);

              const lcm = prev.messages;
              const newMessages = [...lcm.slice(0, lcm.length - 1)];

              const last = lcm[lcm.length - 1];
              last.content += completion.delta;
              last.id = completion.responseId;

              newMessages[newMessages.length - 1].id = completion.requestId;
              newMessages.push(last);

              return { id: completion.conversationId, name: completion.name, messages: newMessages };
            });
          } else if (response.payload.method === 'Ping' && connectionStatus !== 'connected') {
            setConnectionStatus('connected');
          } else if (response.payload.method === 'ConversationList') {
            const conversationList = ConversationListResponseSchema.parse(response.payload);
            setConversations(conversationList.conversations);
          } else if (response.payload.method === 'Load') {
            const conversation = ConversationSchema.parse(response.payload);
            setLoadedConversation(conversation);
          } else if (response.payload.method === 'SystemPrompt') {
            setSystemPrompt(response.payload.content);
          }
        } catch (error) {
          console.log(error);
        }
      };

      ws.onerror = (event) => {
        setError(new Error('WebSocket error occurred'));
      };

      ws.onclose = () => {
        setConnectionStatus('disconnected');
        setSocket(null);

        if (retryCount < maxRetries) {
          setTimeout(() => {
            setRetryCount(prev => prev + 1);
            connect();
          }, retryInterval);
        }
      };

      setSocket(ws);
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Failed to create WebSocket connection'));
    }
  }, [url, retryCount, maxRetries, retryInterval]);

  const sendMessage = useCallback((message: ArrakisRequest) => {
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(typeof message === 'string' ? message : JSON.stringify(message));
    } else {
      console.error('WebSocket is not connected');
    }
  }, [socket]);

  useEffect(() => {
    connect();

    return () => {
      if (socket) {
        socket.close();
      }
    };
  }, [connect]);

  return {
    socket,
    systemPrompt,
    conversations,
    loadedConversation,
    setLoadedConversation,
    sendMessage,
    connectionStatus,
    error
  };
};

interface Sizing {
  value: number;
  unit: string;
  toString(): string;
}

function createSizing(value: number, unit: string): Sizing {
  return {
    value, unit, toString: () => { return `${value}${unit}`; }
  };
}

type ReactElementOrText = React.ReactElement | string | null;
function htmlToReactElements(htmlString: string) {
  const parser = new DOMParser();
  const doc = parser.parseFromString(htmlString, 'text/html');

  function domToReact(node: Node): ReactElementOrText {
    if (node.nodeType === Node.TEXT_NODE) {
      return node.textContent;
    }

    if (node.nodeType === Node.ELEMENT_NODE) {
      const elementNode = node as HTMLElement;

      const tagName = (() => {
        const tn = elementNode.tagName.toLowerCase();
        return tn;
      })();

      const props: Record<string, string> = {};
      Array.from(elementNode.attributes).forEach(attr => {
        let name = attr.name;
        if (name === 'class') name = 'className';
        if (name === 'for') name = 'htmlFor';

        props[name] = attr.value;
      });

      const children = Array.from(elementNode.childNodes).map(domToReact);

      return React.createElement(tagName, props, ...children);
    }

    return null;
  }

  return Array.from(doc.body.childNodes).map(domToReact);
}

const escapeToHTML: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;'
};

const escapeFromHTML: Record<string, string> = Object.entries(escapeToHTML).reduce((acc, [key, value]) => {
  acc[value as string] = key;
  return acc;
}, {} as Record<string, string>);

const PopupButton = (props: { model: string, modelCallback: Function }) => {
  const [isOpen, setIsOpen] = useState(false);
  const popupRef = useRef<HTMLDivElement | null>(null);
  const buttonRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent | any) => {
      if (
        popupRef.current &&
        !popupRef.current.contains(event.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside as any);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside as any);
    };
  }, []);

  return (
    <div
      ref={buttonRef}
      onClick={() => setIsOpen(!isOpen)}
      className="buttonHoverLight"
      style={{
        padding: '0.5rem',
        userSelect: 'none',
        cursor: 'pointer',
        border: '1px solid #EDEDED',
        borderRadius: '0.25rem',
      }}>
      {props.model}

      {
        isOpen && (
          <div
            ref={popupRef}
            className="popup-content"
          >
            {
              [
                { model: "gpt-4o", provider: "openai" },
                { model: "gpt-4o-mini", provider: "openai" },
                { model: "o1-preview", provider: "openai" },
                { model: "o1-mini", provider: "openai" },
                { model: "llama3-70b-8192", provider: "groq" },
                { model: "claude-3-opus-20240229", provider: "anthropic" },
                { model: "claude-3-sonnet-20240229", provider: "anthropic" },
                { model: "claude-3-haiku-20240307", provider: "anthropic" },
                { model: "claude-3-5-sonnet-latest", provider: "anthropic" },
                { model: "claude-3-5-haiku-latest", provider: "anthropic" }
              ].map(m => (
                <div
                  onClick={() => props.modelCallback(m)}
                  className="buttonHover"
                  style={{
                    textWrap: 'nowrap',
                    padding: '0.5rem',
                  }}>
                  {m.model}
                </div>
              ))
            }
          </div>
        )
      }
    </div >
  );
};

type Modal = 'search' | 'none';

function MainPage() {
  const {
    connectionStatus,
    systemPrompt,
    conversations,
    loadedConversation,
    setLoadedConversation,
    sendMessage,
  } = useWebSocket({
    url: 'ws://localhost:9001',
    retryInterval: 5000,
    maxRetries: 0
  });

  // Triggered/set when a modal is selected from the menu buttons on the left side of the screen
  const [selectedModal, setSelectedModal] = useState<Modal | null>(null);

  // TODO: deprecated--this isn't being used anymore
  const [mouseInChat, setMouseInChat] = useState<boolean>(false);

  // This represents the model to be used to generate the next message in the conversation
  // Chosen through the dropdown (jump up?) menu in the bottom left
  const [model, setModel] = useState<API>({ provider: 'anthropic', model: 'claude-3-5-sonnet-latest' });

  // The conversation title card in the top left
  // TODO: this is a little unstable and needs debugging when conversation names are changing around
  const titleDefault = () => ({ title: '', index: 0 });
  const [displayedTitle, setDisplayedTitle] = useState<{ title: string; index: number; }>(titleDefault());

  // TODO: ???
  //       this is unbelievably stupid
  const [inputSizings, _] = useState({
    height: createSizing(0, 'px'),
    padding: createSizing(0.75, 'em'),
    margin: createSizing(1, 'em'),
  });

  // This is really only used to scroll the chat down to the bottom when a message is being streamed
  const messagesRef = useRef() as React.MutableRefObject<HTMLDivElement>;

  // TODO: This is remarkably terrible
  //       This is supposed to be a sort of GET request to get the system prompt
  //       but the nature and presentation of it needs cleaned up badly
  useEffect(() => {
    if (connectionStatus === 'connected') {
      sendMessage({
        method: 'SystemPrompt',
        payload: {
          write: false,
          content: '',
        } satisfies SystemPromptRequest
      } satisfies ArrakisRequest);

    }
  }, [connectionStatus]);

  useEffect(() => {
    const handleKeyPress = (event: any) => {
      if (selectedModal) {
        // TODO: deprecated condition
        //       This is primarily to have chat input automatically focus when the user starts typing
        //       regardless of where the user cursor/focus is
        if (mouseInChat) {
          (document.getElementById('chatInput') as HTMLInputElement).focus();
          return;
        } else {
          // TODO: actually do something with search here
          //       or drop this entirely
          //       do something please
          if (selectedModal !== 'search') {
            (document.getElementById(selectedModal === 'search' ? 'searchInput' : (selectedModal === 'systemPrompt' ? 'promptInput' : '')) as HTMLInputElement).focus();
            return;
          }
        }
      }

      // We don't want to do anything if just control is pressed
      if (event.ctrlKey && event.key !== 'v') {
        return;
      }

      // Keep a newline from being input when a message is trying to be sent
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
      }

      // Finally at the point where we want to actually focus the input
      (document.getElementById('chatInput') as HTMLInputElement).focus();
    }

    document.addEventListener('keydown', handleKeyPress);

    return () => {
      document.removeEventListener('keydown', handleKeyPress);
    };
  }, [selectedModal, mouseInChat]);

  function isGuid(str: string): boolean {
    const guidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
    return guidRegex.test(str);
  }

  useEffect(() => {
    // Scroll the conversation to the bottom while streaming the messages
    if (messagesRef.current) {
      messagesRef.current.scrollTo({
        top: messagesRef.current.scrollHeight,
        behavior: 'auto'
      });
    }

    // Loading the conversation name
    // This incrementally loads each character of the conversation name
    // to mimic a typing motion
    let intervalId: any = null;
    if (!isGuid(loadedConversation.name)) {
      intervalId = setInterval(() => {
        const conversationName = loadedConversation.name;

        if (displayedTitle.index < conversationName.length) {
          setDisplayedTitle(prev => {
            if (prev.index < conversationName.length) {
              return { title: prev.title + conversationName[prev.index], index: prev.index + 1 };
            } else {
              return prev;
            }
          });
        } else {
          clearInterval(intervalId);
        }
      }, 50);
    }

    return () => {
      if (intervalId) {
        clearInterval(intervalId);
      }
    };
  }, [loadedConversation]);

  // This is the chat message submit
  // Takes care of the logic of:
  // - Sending the conversation through William's backend
  // - Updating the UI with the new message
  const sendPrompt = (e: any) => {
    const inputElement = document.getElementById('chatInput') as HTMLDivElement;
    if (e.key === 'Enter') {
      // This is a message submission, not a newline
      if (!e.shiftKey) {
        e.preventDefault();
        const data = inputElement.innerText;

        // We don't want to allow empty message submissions
        if (data.length === 0) {
          return;
        }

        // Update the local conversation message array
        // Current logic is to send an empty placeholder message for the Assistant
        // This is what gets populated by the response
        const messages = loadedConversation.messages;
        const newMessages = [
          ...messages,
          {
            id: messages.length > 0 ? messages[messages.length - 1].id! + 1 : null,
            content: data,
            message_type: 'User',
            api: model,
            system_prompt: '',
            sequence: messages.length
          } satisfies Message,
          {
            id: messages.length > 0 ? messages[messages.length - 1].id! + 2 : null,
            content: '',
            message_type: 'Assistant',
            api: model,
            system_prompt: '',
            sequence: messages.length + 1
          } satisfies Message,
        ];

        const newConversation = {
          ...loadedConversation,
          messages: newMessages,
        };

        // State update
        setLoadedConversation(newConversation);

        // Send the updated conversation to the backend
        sendMessage({
          method: 'Completion',
          payload: newConversation,
        } satisfies ArrakisRequest);

        // Reset the chat input
        inputElement.innerHTML = '';
      }

      // The newline case is handled implicitly here
    }
  };

  // Ping to see if we're connected to the backend
  // TODO: I don't think this should really be here
  //       The disconnection from the backend has some pretty immediate feedback
  useEffect(() => {
    const pingInterval = setInterval(() => {
      sendMessage(ArrakisRequestSchema.parse({
        method: 'Ping',
        payload: {
          body: 'ping',
        } satisfies PingRequest,
      }));
    }, 5000);

    // Clean up interval when component unmounts
    return () => clearInterval(pingInterval);
  }, [sendMessage]);

  // Whenever a modal is selected, get a list of the saved conversations from the backend
  // TODO: this could probably be cleaned up to be only when the History modal is loaded
  useEffect(() => {
    sendMessage({
      method: 'ConversationList',
    } satisfies ArrakisRequest);
  }, [selectedModal]);

  // Decide which modal to build + return based on the currently selected modal
  const getModal = () => {
    if (selectedModal === 'search') {
      // Callback to load a given conversation based on its ID
      // Closes the modal, updates the title, and fetches the conversation from the backend
      const getConversationCallback = (id: number) => {
        return () => {
          setDisplayedTitle(titleDefault());
          setSelectedModal(null);
          sendMessage({
            method: 'Load',
            payload: {
              id,
            } satisfies LoadRequest
          } satisfies ArrakisRequest);
        };
      };

      // Build the modal HTML with the listed conversations
      return (
        <div style={{
          margin: '0.5rem',
          display: 'flex',
          flexDirection: 'column',
          maxHeight: 'calc(100vh - 1rem)',
          overflowY: 'auto',
          overflowX: 'hidden',
        }}>
          {conversations.map(c => {
            return (
              <div className="buttonHoverLight" onClick={getConversationCallback(c.id!)} style={{
                padding: '0.5rem',
                cursor: 'pointer',
                userSelect: 'none',
                borderRadius: '0.5rem',
                textWrap: 'nowrap',
              }}>
                {formatTitle(c.name)}
              </div>
            );
          })}
        </div>
      );
    }
    // Show the system prompt textarea for reading/writing
    else if (selectedModal === 'systemPrompt') {
      return (
        <div style={{
          margin: '0.5rem',
          display: 'flex',
          flexDirection: 'column',
        }}>
          <textarea id="promptInput" placeholder="You are a helpful assistant." style={{
            border: 0,
            position: 'relative',
            top: '1px',
            outline: 0,
            resize: 'none',
            height: '45vh',
            borderRadius: '0.5rem',
            fontSize: '16px',
            padding: '0.5rem',
            marginBottom: '0.5rem',
            textWrap: 'nowrap',
          }} onBlur={() => {
            // The system prompt is updated on the backend + stored to disk
            // _only_ when the user unfocuses the textarea
            // TODO: this needs changed--it's unintuitive
            sendMessage({
              method: 'SystemPrompt',
              payload: {
                write: true,
                content: (document.getElementById('promptInput')! as HTMLTextAreaElement).value,
              } satisfies SystemPromptRequest
            } satisfies ArrakisRequest);
          }}>{systemPrompt}</textarea>
        </div>
      );
    }
  };

  // Function for parsing a string + adding Latex math delimiters for Markdown-It-Katex to properly render
  const addMathDelimiters = (input: string) => {
    const openChars = ['\\(', '\\['];
    const closeChars = ['\\)', '\\]'];

    let result = '';
    let currentIndex = 0;

    while (currentIndex < input.length) {
      // Find next opening character
      let foundOpenChar = false;
      let openCharIndex = -1;
      let matchedCloseChar = '';

      for (let i = 0; i < openChars.length; i++) {
        const index = input.indexOf(openChars[i], currentIndex);
        if (index !== -1 && (openCharIndex === -1 || index < openCharIndex)) {
          openCharIndex = index;
          matchedCloseChar = closeChars[i];
          foundOpenChar = true;
        }
      }

      if (!foundOpenChar) {
        // Add remaining text and break
        result += input.slice(currentIndex);
        break;
      }

      // Add text before the math content
      result += input.slice(currentIndex, openCharIndex);

      // Find closing character
      const closeCharIndex = input.indexOf(matchedCloseChar, openCharIndex + 2);

      // Extract and process math content
      if (closeCharIndex === -1) {
        // No closing character found
        const mathContent = input.slice(openCharIndex + 2);
        const hasNewline = mathContent.includes('\n');
        result += hasNewline ? '$$' + mathContent + '$$' : '$' + mathContent + '$';
        break;
      } else {
        // Closing character found
        const mathContent = input.slice(openCharIndex + 2, closeCharIndex).trim();
        const hasNewline = mathContent.includes('\n');
        result += hasNewline ? '$$' + mathContent + '$$' : '$' + mathContent + '$';
        currentIndex = closeCharIndex + 2;
      }
    }

    return result;
  };

  const menuButtonStyle: React.CSSProperties = {
    userSelect: 'none',
    cursor: 'pointer',
    width: '100%',
    height: '2rem',
    alignSelf: 'center',
    display: 'flex',
    justifyContent: 'center',
    alignItems: 'center',
    textAlign: 'center',
    margin: '0.25rem 0',
    borderRadius: '0.5rem',
  };

  // Main window component
  return (
    <div style={{
      height: '100vh',
      display: 'flex',
      flexDirection: 'row',
    }}>
      {
        /*
         * TODO: Deprecated mouseInChat nonsense
         *
         * This is the main chat component
         */
      }
      <div ref={messagesRef} onMouseEnter={() => setMouseInChat(true)} onMouseLeave={() => setMouseInChat(false)} style={{
        height: 'calc(100vh - 1rem)',
        display: 'flex',
        flexDirection: 'column',
        width: 'calc(100% - 1rem)',
        overflowY: 'auto',
        border: '1px solid #E0DED9',
        boxShadow: '0 0 8px rgba(28, 25, 23, 0.1)',
        margin: '0.5rem',
        backgroundColor: '#F9F8F7',
        borderRadius: '0.5rem',
      }}>
        {
          /*
           * This is the main wrapper around the chat input that places it in the center of the screen
           * TODO: revisit how much of this is actually needed
           */
        }
        <div
          style={{
            position: 'fixed',
            left: 'calc(50% - 0.375rem)',
            transform: 'translateX(-50%)',
            bottom: '1rem',
            width: '35vw',
            minHeight: '1rem',
            padding: inputSizings.padding.toString(),
            backgroundColor: '#EFECEA',
            borderRadius: '0.5rem',
            fontSize: '16px',
            overflow: 'hidden',
            display: 'flex',
          }}
        >
          <div style={{
            maxHeight: '25vh',
            overflow: 'auto',
            height: '100%',
            width: '100%',
          }}>
            <div
              contentEditable={true}
              id="chatInput"
              onKeyDown={sendPrompt}
              style={{
                height: '100%',
                width: '100%',
                border: 0,
                outline: 0,
                resize: 'none',
                alignSelf: 'center',
                backgroundColor: 'transparent',
              }}
            />
          </div>
        </div>
        <div style={{
          position: 'relative',
          width: '40vw',
          margin: '0 auto',
          flex: 1,
          paddingBottom: `calc(${inputSizings.height.toString()} + 10vh)`
        }}>
          {loadedConversation.messages.map((m) => {
            // Lot of preprocessing here to properly render the messages into markdown into react components

            const toPattern = new RegExp(
              Object.keys(escapeToHTML)
                .map(key => key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
                .join('|'),
              'g'
            );

            // Escaping the incoming messages to... TODO: what?
            let content = m.content.replace(toPattern, function (match) {
              return escapeToHTML[match];
            });

            // Markdown to HTML conversion
            content = addMathDelimiters(content);
            content = md.render(content) as string;

            const reactElements = htmlToReactElements(content);

            // Processing each react element to clean the contained text
            // and fix up the CSS styles to be camelCase
            function modifyElements(element: any): ReactElementOrText {
              if (typeof element === 'string') {
                let c = element as string;
                const fromPattern = new RegExp(
                  Object.keys(escapeFromHTML)
                    .map(key => key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
                    .join('|'),
                  'g'
                );

                return c.replace(fromPattern, function (match) {
                  return escapeFromHTML[match];
                })
              }

              const props = element.props as React.PropsWithChildren<{ [key: string]: any }>;

              const input = props.style;

              if (input) {
                if (typeof input !== "string") return null;

                const styleObject: { [key: string]: any } = {};
                const styleEntries = input.split(";").filter(Boolean);

                for (const entry of styleEntries) {
                  const [property, value] = entry.split(":").map((s) => s.trim());
                  if (property && value) {
                    // Convert CSS property to camelCase for React style
                    const camelCaseProperty = property.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
                    styleObject[camelCaseProperty] = value;
                  }
                }

                return React.cloneElement(element, {
                  style: styleObject,
                  children: React.Children.map(props.children, (child) =>
                    React.isValidElement(child) ? modifyElements(child) : child
                  ),
                });
              }

              if (props.children) {
                return React.createElement(
                  element.type,
                  element.props,
                  React.Children.map(props.children, modifyElements)
                );
              }

              return element;
            };

            const unescapedElements = reactElements.map(modifyElements);

            const isUser = m.message_type === 'User';

            // Actually build the component which holds the chat history + all the contained messages
            return (
              <>
                <div style={{
                  color: '#ABA7A2',
                  fontSize: '0.7rem',
                  marginTop: '1rem',
                  marginLeft: '0.5rem',
                  userSelect: 'none',
                  marginBottom: isUser ? '' : '-0.25rem'
                }}>{isUser ? 'You' : model.model}</div>
                <div style={{
                  backgroundColor: isUser ? '#E2E0DD' : '',
                  borderRadius: '0.5rem',
                  margin: '0 0.25rem 0.25rem 0.25rem 0.25rem',
                  padding: '0.01rem 0',
                  width: isUser ? 'fit-content' : '',
                  position: 'relative'
                }}>
                  {isUser ? '' : (
                    <p className="messageOptions" style={{
                      position: 'absolute',
                      transform: 'translateX(calc(-100% - 1rem))',
                      userSelect: 'none',
                      cursor: 'pointer',
                      display: 'flex',
                    }}>
                      <div style={{
                        width: 'fit-content',
                        overflow: 'hidden',
                      }}>
                        <div className="messageOptionsRow">
                          <div style={{
                            padding: '0 0.5rem',
                          }} onClick={() => {
                            // Regeneration option for a given message
                            // This forks the existing conversation and saves the new conversation as a new entry in the listing
                            // All conversation up to the regenerated message is kept, all conversation history after is left behind in the old conversation

                            sendMessage(ArrakisRequestSchema.parse({
                              method: 'Fork',
                              payload: ForkRequestSchema.parse({
                                conversationId: loadedConversation.id,
                                sequence: m.sequence
                              })
                            }));

                            const conversation = {
                              ...loadedConversation,
                              messages: loadedConversation.messages.slice(0, m.sequence + 1),
                            };

                            let last = conversation.messages[conversation.messages.length - 1];

                            // Cleaning message metadata for the new conversation entry
                            last.content = '';
                            last.id = null;
                            last.message_type = 'Assistant';
                            last.system_prompt = systemPrompt;
                            last.api = model;

                            conversation.messages[conversation.messages.length - 1] = last;

                            setLoadedConversation(conversation);
                          }}>Regenerate</div>
                        </div>
                      </div>
                      <div>•</div>
                    </p>
                  )}

                  {
                    /* The actual message elements */
                    unescapedElements
                  }
                </div>
              </>
            );
          })}
        </div>

        { /* Current conversation title and general interaction options */ }
        <div style={{
          position: 'absolute',
          width: '5vw',
          display: 'flex',
          flexDirection: 'column',
          backgroundColor: 'transparent',
          margin: '0.5rem 0 0.5rem 0',
        }}>

          { /* Current loaded conversation title */ }
          <div style={{
            fontWeight: 'bold',
            height: '1rem',
            textWrap: 'nowrap',
            margin: '0.25rem 0 1rem 0.5rem',
          }}>{formatTitle(displayedTitle.title)}</div>

          { /* Element to determine whether the frontend has an established websocket connection with the backend */ }
          <div style={{
            backgroundColor: connectionStatus === 'disconnected' ? 'red' : '#56F55E',
            userSelect: 'none',
            width: '24px',
            height: '24px',
            margin: '0.5rem',
            borderRadius: '0.5rem',
            alignSelf: 'center',
          }} />

          { /* Create a new conversation and clear the current conversation history */ }
          <div className="buttonHoverLight" onClick={() => {
            setSelectedModal(null);
            setLoadedConversation(conversationDefault());
            setDisplayedTitle(titleDefault());
          }} style={menuButtonStyle}>New</div>

          { /* View + give the option to load saved conversations */ }
          <div className="buttonHoverLight" onClick={() => setSelectedModal(selectedModal !== 'search' ? 'search' : null)} style={menuButtonStyle}>History</div>

          { /* Display element for the selected modal, if any */ }
          <div style={{
            display: selectedModal ? '' : 'none',
          }}>
            <div style={{
              position: 'fixed',
              left: 0,
              top: 0,
              height: '100vh',
              width: '100vw',
              backgroundColor: '#00000010',
            }} onClick={() => setSelectedModal('')} />
            <div style={{
              position: 'fixed',
              backgroundColor: '#F9F8F7',
              width: '55vw',
              height: '45vh',
              top: '50%',
              left: '50%',
              transform: 'translate(-50%, -50%)',
              overflow: 'hidden',
              borderRadius: '1rem',
            }}>
              {getModal()}
            </div>
          </div>
        </div>

        { /* Pop-up menu for selecting the LLM backend to use */ }
        <div style={{
          position: 'sticky',
          marginLeft: '0.5rem',
          bottom: '0.5rem',
          width: 'fit-content',
        }}>
          <PopupButton model={model.model} modelCallback={setModel} />
        </div>
      </div>
    </div >
  );
}

root.render(
  <MainPage />
);
import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
    Timer,
    RotateCcw,
    Save,
    Check,
    Sparkles,
    UploadCloud,
    RadioTower,
    Files,
    ServerCog,
} from "lucide-react";
import { StreamTimeoutConfig, UploadTimeoutConfig } from "../../types/config";
import { showToast } from "../common/ToastContainer";

interface TimeoutSettingsProps {
    streamConfig?: StreamTimeoutConfig;
    uploadConfig?: UploadTimeoutConfig;
    onStreamChange: (config: StreamTimeoutConfig) => void;
    onUploadChange: (config: UploadTimeoutConfig) => void;
    onSave?: () => Promise<void> | void;
}

const DEFAULT_STREAM: Required<StreamTimeoutConfig> = {
    initial_ttft_secs: 180,
    transition_secs: 180,
    streaming_sliding_secs: 120,
};

const DEFAULT_UPLOAD: Required<UploadTimeoutConfig> = {
    connect_secs: 10,
    min_upload_mbps: 2,
    extra_wait_ratio: 0.4,
    stall_secs: 45,
    header_wait_secs: 0,
    large_body_threshold_mb: 8,
    large_body_endpoint_attempts: 1,
};

type UploadField =
    | "connect_secs"
    | "min_upload_mbps"
    | "extra_wait_ratio"
    | "stall_secs"
    | "header_wait_secs"
    | "large_body_threshold_mb"
    | "large_body_endpoint_attempts";

type StreamField = "initial_ttft_secs" | "transition_secs" | "streaming_sliding_secs";

export const TimeoutSettings: React.FC<TimeoutSettingsProps> = ({
    streamConfig,
    uploadConfig,
    onStreamChange,
    onUploadChange,
    onSave,
}) => {
    const { t } = useTranslation();
    const [isSaving, setIsSaving] = useState(false);
    const [savedSuccessfully, setSavedSuccessfully] = useState(false);

    const [uploadRaw, setUploadRaw] = useState<Record<UploadField, string>>(() => ({
        connect_secs: String(uploadConfig?.connect_secs ?? DEFAULT_UPLOAD.connect_secs),
        min_upload_mbps: String(uploadConfig?.min_upload_mbps ?? DEFAULT_UPLOAD.min_upload_mbps),
        extra_wait_ratio: String(uploadConfig?.extra_wait_ratio ?? DEFAULT_UPLOAD.extra_wait_ratio),
        stall_secs: String(uploadConfig?.stall_secs ?? DEFAULT_UPLOAD.stall_secs),
        header_wait_secs: String(uploadConfig?.header_wait_secs ?? DEFAULT_UPLOAD.header_wait_secs),
        large_body_threshold_mb: String(uploadConfig?.large_body_threshold_mb ?? DEFAULT_UPLOAD.large_body_threshold_mb),
        large_body_endpoint_attempts: String(uploadConfig?.large_body_endpoint_attempts ?? DEFAULT_UPLOAD.large_body_endpoint_attempts),
    }));

    const [streamRaw, setStreamRaw] = useState<Record<StreamField, string>>(() => ({
        initial_ttft_secs: String(streamConfig?.initial_ttft_secs ?? DEFAULT_STREAM.initial_ttft_secs),
        transition_secs: String(streamConfig?.transition_secs ?? DEFAULT_STREAM.transition_secs),
        streaming_sliding_secs: String(streamConfig?.streaming_sliding_secs ?? DEFAULT_STREAM.streaming_sliding_secs),
    }));

    useEffect(() => {
        setUploadRaw({
            connect_secs: String(uploadConfig?.connect_secs ?? DEFAULT_UPLOAD.connect_secs),
            min_upload_mbps: String(uploadConfig?.min_upload_mbps ?? DEFAULT_UPLOAD.min_upload_mbps),
            extra_wait_ratio: String(uploadConfig?.extra_wait_ratio ?? DEFAULT_UPLOAD.extra_wait_ratio),
            stall_secs: String(uploadConfig?.stall_secs ?? DEFAULT_UPLOAD.stall_secs),
            header_wait_secs: String(uploadConfig?.header_wait_secs ?? DEFAULT_UPLOAD.header_wait_secs),
            large_body_threshold_mb: String(uploadConfig?.large_body_threshold_mb ?? DEFAULT_UPLOAD.large_body_threshold_mb),
            large_body_endpoint_attempts: String(uploadConfig?.large_body_endpoint_attempts ?? DEFAULT_UPLOAD.large_body_endpoint_attempts),
        });
    }, [
        uploadConfig?.connect_secs,
        uploadConfig?.min_upload_mbps,
        uploadConfig?.extra_wait_ratio,
        uploadConfig?.stall_secs,
        uploadConfig?.header_wait_secs,
        uploadConfig?.large_body_threshold_mb,
        uploadConfig?.large_body_endpoint_attempts,
    ]);

    useEffect(() => {
        setStreamRaw({
            initial_ttft_secs: String(streamConfig?.initial_ttft_secs ?? DEFAULT_STREAM.initial_ttft_secs),
            transition_secs: String(streamConfig?.transition_secs ?? DEFAULT_STREAM.transition_secs),
            streaming_sliding_secs: String(streamConfig?.streaming_sliding_secs ?? DEFAULT_STREAM.streaming_sliding_secs),
        });
    }, [streamConfig?.initial_ttft_secs, streamConfig?.transition_secs, streamConfig?.streaming_sliding_secs]);

    const parseUpload = (field: UploadField): number => {
        const raw = uploadRaw[field];
        const val =
            field === "min_upload_mbps" || field === "extra_wait_ratio"
                ? parseFloat(raw)
                : parseInt(raw, 10);
        return isNaN(val) ? DEFAULT_UPLOAD[field] : val;
    };

    const parseStream = (field: StreamField): number => {
        const val = parseInt(streamRaw[field], 10);
        return isNaN(val) ? DEFAULT_STREAM[field] : val;
    };

    const mergedUpload = (overrides: Partial<UploadTimeoutConfig> = {}): UploadTimeoutConfig => ({
        connect_secs: parseUpload("connect_secs"),
        min_upload_mbps: parseUpload("min_upload_mbps"),
        extra_wait_ratio: parseUpload("extra_wait_ratio"),
        stall_secs: parseUpload("stall_secs"),
        header_wait_secs: parseUpload("header_wait_secs"),
        large_body_threshold_mb: parseUpload("large_body_threshold_mb"),
        large_body_endpoint_attempts: parseUpload("large_body_endpoint_attempts"),
        ...overrides,
    });

    const mergedStream = (overrides: Partial<StreamTimeoutConfig> = {}): StreamTimeoutConfig => ({
        initial_ttft_secs: parseStream("initial_ttft_secs"),
        transition_secs: parseStream("transition_secs"),
        streaming_sliding_secs: parseStream("streaming_sliding_secs"),
        ...overrides,
    });

    // 统一首字延迟（TTFT）派生：媒体类型只保留相对增量，不再维护独立绝对超时表。
    // 与后端 upstream::client 的 first_byte_offset_secs / MAX_FIRST_BYTE_* 保持一致。
    const TTFT_BASE_CAP = 150;
    const TTFT_CEILING = 210;
    const ttftBaseSecs = (() => {
        const raw = Number(streamRaw.initial_ttft_secs);
        const value = Number.isFinite(raw) && raw > 0 ? raw : DEFAULT_STREAM.initial_ttft_secs;
        return Math.min(Math.max(value, 15), TTFT_BASE_CAP);
    })();
    const clampFirstByte = (v: number) => Math.min(Math.max(v, 15), TTFT_CEILING);
    const firstByteText = clampFirstByte(ttftBaseSecs);
    const firstByteImage = clampFirstByte(ttftBaseSecs + 45);
    const firstByteVideo = clampFirstByte(ttftBaseSecs + 60);
    const firstByteAux = clampFirstByte(ttftBaseSecs - 30);

    const handleUploadInput = (field: UploadField, text: string) => {
        const isDecimal = field === "min_upload_mbps" || field === "extra_wait_ratio";
        let filtered = isDecimal ? text.replace(/[^0-9.]/g, "") : text.replace(/[^0-9]/g, "");
        if (isDecimal && filtered.includes(".")) {
            const parts = filtered.split(".");
            filtered = `${parts[0]}.${parts.slice(1).join("")}`;
        }
        setUploadRaw((prev) => ({ ...prev, [field]: filtered }));
        if (filtered !== "" && filtered !== ".") {
            const num = isDecimal ? parseFloat(filtered) : parseInt(filtered, 10);
            if (!isNaN(num)) {
                onUploadChange(mergedUpload({ [field]: num } as Partial<UploadTimeoutConfig>));
            }
        }
    };

    const handleStreamInput = (field: StreamField, text: string) => {
        const filtered = text.replace(/[^0-9]/g, "");
        setStreamRaw((prev) => ({ ...prev, [field]: filtered }));
        if (filtered !== "") {
            const num = parseInt(filtered, 10);
            if (!isNaN(num) && num > 0) {
                onStreamChange(mergedStream({ [field]: num } as Partial<StreamTimeoutConfig>));
            }
        }
    };

    const handleUploadBlur = (field: UploadField) => {
        if (uploadRaw[field] === "") {
            setUploadRaw((prev) => ({ ...prev, [field]: String(DEFAULT_UPLOAD[field]) }));
            onUploadChange(mergedUpload({ [field]: DEFAULT_UPLOAD[field] } as Partial<UploadTimeoutConfig>));
        }
    };

    const handleStreamBlur = (field: StreamField) => {
        if (streamRaw[field] === "" || parseInt(streamRaw[field], 10) <= 0) {
            setStreamRaw((prev) => ({ ...prev, [field]: String(DEFAULT_STREAM[field]) }));
            onStreamChange(mergedStream({ [field]: DEFAULT_STREAM[field] } as Partial<StreamTimeoutConfig>));
        }
    };

    // 最大等待时间 = 请求体大小 ÷ 上传带宽 × (1 + 额外等待系数)
    const estimateUploadSecs = (mb: number): number => {
        const mbps = Math.max(parseFloat(uploadRaw.min_upload_mbps) || DEFAULT_UPLOAD.min_upload_mbps, 0.1);
        const ratio = Math.max(parseFloat(uploadRaw.extra_wait_ratio) || DEFAULT_UPLOAD.extra_wait_ratio, 0);
        const bytesPerSec = (mbps * 1_000_000) / 8;
        const transferSecs = (mb * 1_000_000) / bytesPerSec;
        return Math.round(transferSecs * (1 + ratio));
    };

    const handleResetDefaults = () => {
        setUploadRaw({
            connect_secs: String(DEFAULT_UPLOAD.connect_secs),
            min_upload_mbps: String(DEFAULT_UPLOAD.min_upload_mbps),
            extra_wait_ratio: String(DEFAULT_UPLOAD.extra_wait_ratio),
            stall_secs: String(DEFAULT_UPLOAD.stall_secs),
            header_wait_secs: String(DEFAULT_UPLOAD.header_wait_secs),
            large_body_threshold_mb: String(DEFAULT_UPLOAD.large_body_threshold_mb),
            large_body_endpoint_attempts: String(DEFAULT_UPLOAD.large_body_endpoint_attempts),
        });
        setStreamRaw({
            initial_ttft_secs: String(DEFAULT_STREAM.initial_ttft_secs),
            transition_secs: String(DEFAULT_STREAM.transition_secs),
            streaming_sliding_secs: String(DEFAULT_STREAM.streaming_sliding_secs),
        });
        onUploadChange({ ...DEFAULT_UPLOAD });
        onStreamChange({ ...DEFAULT_STREAM });
        showToast(t("proxy.upload_timeouts.toast_reset", { defaultValue: "已恢复超时默认值" }), "info");
    };

    const handleSave = async () => {
        setIsSaving(true);
        try {
            if (onSave) {
                await onSave();
            }
            setSavedSuccessfully(true);
            showToast(t("common.success", { defaultValue: "设置已保存" }), "success");
            setTimeout(() => setSavedSuccessfully(false), 2000);
        } catch (error) {
            console.error("保存超时设置失败:", error);
            showToast(t("common.error", { defaultValue: "保存失败" }), "error");
        } finally {
            setIsSaving(false);
        }
    };

    const numberField = (
        value: string,
        placeholder: number,
        unit: string,
        onChange: (text: string) => void,
        onBlur: () => void,
    ) => (
        <div className="flex items-center gap-2">
            <input
                type="text"
                inputMode="decimal"
                placeholder={String(placeholder)}
                value={value}
                onChange={(e) => onChange(e.target.value)}
                onBlur={onBlur}
                className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-100 text-xs font-mono font-semibold text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500"
            />
            <span className="text-xs text-gray-500 font-medium shrink-0">{unit}</span>
        </div>
    );

    const fieldCard = (
        icon: React.ReactNode,
        title: string,
        desc: string,
        inputs: React.ReactNode,
    ) => (
        <div className="p-3.5 bg-gray-50/70 dark:bg-base-200/60 rounded-xl border border-gray-200/70 dark:border-base-300 flex flex-col justify-between space-y-3">
            <div className="space-y-1.5">
                <span className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5">
                    {icon}
                    {title}
                </span>
                <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-normal">{desc}</p>
            </div>
            <div className="space-y-2">{inputs}</div>
        </div>
    );

    const sectionTitle = (icon: React.ReactNode, text: string) => (
        <div className="flex items-center gap-1.5 pt-1 text-[11px] font-bold uppercase tracking-wide text-gray-500 dark:text-gray-400">
            {icon}
            {text}
        </div>
    );

    const seconds = t("common.seconds", { defaultValue: "秒" });

    return (
        <div className="space-y-4 text-gray-800 dark:text-gray-200">
            {/* 顶栏说明 */}
            <div className="p-3 bg-sky-50/70 dark:bg-sky-950/20 rounded-xl border border-sky-200/70 dark:border-sky-800/40 text-xs leading-relaxed space-y-1">
                <div className="font-semibold text-sky-900 dark:text-sky-300 flex items-center gap-1.5">
                    <Sparkles size={14} className="text-sky-600 dark:text-sky-400" />
                    {t("proxy.timeouts.banner_title")}
                </div>
                <p className="text-gray-600 dark:text-gray-400">{t("proxy.timeouts.banner_desc")}</p>
            </div>

            {/* ① 端点建立连接 */}
            {sectionTitle(<ServerCog size={13} className="text-cyan-500" />, t("proxy.timeouts.section_connect"))}
            <div className="grid grid-cols-1 md:grid-cols-3 gap-3.5">
                {fieldCard(
                    <ServerCog size={14} className="text-cyan-500" />,
                    t("proxy.timeouts.connect_label"),
                    t("proxy.timeouts.connect_desc"),
                    <>
                        {numberField(
                            uploadRaw.connect_secs,
                            DEFAULT_UPLOAD.connect_secs,
                            seconds,
                            (text) => handleUploadInput("connect_secs", text),
                            () => handleUploadBlur("connect_secs"),
                        )}
                        <p className="text-[10px] text-amber-600 dark:text-amber-400 leading-normal">
                            {t("proxy.timeouts.connect_restart")}
                        </p>
                    </>,
                )}
            </div>

            {/* ② 上传（发送请求体） */}
            {sectionTitle(<UploadCloud size={13} className="text-blue-500" />, t("proxy.timeouts.section_upload"))}
            <div className="p-3.5 bg-gray-50/70 dark:bg-base-200/60 rounded-xl border border-gray-200/70 dark:border-base-300 space-y-3">
                <div className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5">
                    <UploadCloud size={14} className="text-blue-500" />
                    {t("proxy.upload_timeouts.adaptive_title")}
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                        <div className="text-[11px] font-semibold text-gray-700 dark:text-gray-300 pb-1">
                            {t("proxy.upload_timeouts.min_speed_title")}
                        </div>
                        {numberField(
                            uploadRaw.min_upload_mbps,
                            DEFAULT_UPLOAD.min_upload_mbps,
                            "Mbps",
                            (text) => handleUploadInput("min_upload_mbps", text),
                            () => handleUploadBlur("min_upload_mbps"),
                        )}
                        <p className="text-[10px] text-gray-500 dark:text-gray-400 pt-1 leading-normal">
                            {t("proxy.upload_timeouts.min_speed_desc")}
                        </p>
                    </div>
                    <div>
                        <div className="text-[11px] font-semibold text-gray-700 dark:text-gray-300 pb-1">
                            {t("proxy.upload_timeouts.extra_wait_title")}
                        </div>
                        {numberField(
                            uploadRaw.extra_wait_ratio,
                            DEFAULT_UPLOAD.extra_wait_ratio,
                            "×",
                            (text) => handleUploadInput("extra_wait_ratio", text),
                            () => handleUploadBlur("extra_wait_ratio"),
                        )}
                        <p className="text-[10px] text-gray-500 dark:text-gray-400 pt-1 leading-normal">
                            {t("proxy.upload_timeouts.extra_wait_desc")}
                        </p>
                    </div>
                </div>

                <div className="p-2.5 bg-blue-50/70 dark:bg-blue-950/30 rounded-lg text-[11px] text-blue-900 dark:text-blue-300 space-y-1">
                    <div className="font-semibold">{t("proxy.upload_timeouts.formula_title")}</div>
                    <div className="font-mono">
                        {t("proxy.timeouts.auto_summary", {
                            big: estimateUploadSecs(47),
                        })}
                    </div>
                </div>

                <div>
                    <div className="text-[11px] font-semibold text-gray-700 dark:text-gray-300 pb-1">
                        {t("proxy.upload_timeouts.stall_title")}
                    </div>
                    <div className="max-w-xs">
                        {numberField(
                            uploadRaw.stall_secs,
                            DEFAULT_UPLOAD.stall_secs,
                            seconds,
                            (text) => handleUploadInput("stall_secs", text),
                            () => handleUploadBlur("stall_secs"),
                        )}
                    </div>
                    <p className="text-[10px] text-gray-500 dark:text-gray-400 pt-1 leading-normal">
                        {t("proxy.upload_timeouts.stall_desc")}
                    </p>
                </div>

                {/* 大文件重试 */}
                <div className="pt-2 border-t border-gray-200/60 dark:border-base-300/60">
                    <div className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5 pb-1">
                        <Files size={13} className="text-orange-500" />
                        {t("proxy.timeouts.section_large_body")}
                    </div>
                    <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-normal pb-2">
                        {t("proxy.upload_timeouts.large_body_desc")}
                    </p>
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <div>
                            <div className="text-[11px] font-semibold text-gray-700 dark:text-gray-300 pb-1">
                                {t("proxy.upload_timeouts.large_body_threshold_title")}
                            </div>
                            {numberField(
                                uploadRaw.large_body_threshold_mb,
                                DEFAULT_UPLOAD.large_body_threshold_mb,
                                "MB",
                                (text) => handleUploadInput("large_body_threshold_mb", text),
                                () => handleUploadBlur("large_body_threshold_mb"),
                            )}
                        </div>
                        <div>
                            <div className="text-[11px] font-semibold text-gray-700 dark:text-gray-300 pb-1">
                                {t("proxy.upload_timeouts.endpoint_attempts_title")}
                            </div>
                            {numberField(
                                uploadRaw.large_body_endpoint_attempts,
                                DEFAULT_UPLOAD.large_body_endpoint_attempts,
                                "×",
                                (text) => handleUploadInput("large_body_endpoint_attempts", text),
                                () => handleUploadBlur("large_body_endpoint_attempts"),
                            )}
                        </div>
                    </div>
                </div>
            </div>

            {/* ④ 首字与连续输出 */}
            {sectionTitle(<RadioTower size={13} className="text-purple-500" />, t("proxy.timeouts.section_stream"))}
            <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-normal">
                {t("proxy.timeouts.stream_desc")}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-3.5">
                {fieldCard(
                    <Timer size={14} className="text-blue-500" />,
                    t("proxy.stream_timeouts.ttft_title"),
                    t("proxy.stream_timeouts.ttft_desc"),
                    numberField(
                        streamRaw.initial_ttft_secs,
                        DEFAULT_STREAM.initial_ttft_secs,
                        seconds,
                        (text) => handleStreamInput("initial_ttft_secs", text),
                        () => handleStreamBlur("initial_ttft_secs"),
                    ),
                )}
                {fieldCard(
                    <Timer size={14} className="text-purple-500" />,
                    t("proxy.stream_timeouts.transition_title"),
                    t("proxy.stream_timeouts.transition_desc"),
                    numberField(
                        streamRaw.transition_secs,
                        DEFAULT_STREAM.transition_secs,
                        seconds,
                        (text) => handleStreamInput("transition_secs", text),
                        () => handleStreamBlur("transition_secs"),
                    ),
                )}
                {fieldCard(
                    <Timer size={14} className="text-emerald-500" />,
                    t("proxy.stream_timeouts.sliding_title"),
                    t("proxy.stream_timeouts.sliding_desc"),
                    numberField(
                        streamRaw.streaming_sliding_secs,
                        DEFAULT_STREAM.streaming_sliding_secs,
                        seconds,
                        (text) => handleStreamInput("streaming_sliding_secs", text),
                        () => handleStreamBlur("streaming_sliding_secs"),
                    ),
                )}
            </div>

            {/* 统一首字延迟：媒体类型增量（由 TTFT 派生，不再硬编码独立超时表） */}
            <div className="p-3 bg-purple-50/70 dark:bg-purple-950/20 rounded-lg text-[11px] text-purple-900 dark:text-purple-300 space-y-1">
                <div className="font-semibold">{t("proxy.timeouts.header_primary_title")}</div>
                <ul className="pl-4 list-disc space-y-0.5 text-gray-600 dark:text-gray-400">
                    <li>{t("proxy.timeouts.header_primary_items")}</li>
                </ul>
                <div className="font-semibold pt-1">{t("proxy.timeouts.header_fallback_title")}</div>
                <ul className="pl-4 list-disc space-y-0.5 text-gray-600 dark:text-gray-400">
                    <li>{t("proxy.timeouts.header_fallback_items")}</li>
                </ul>
                <div className="pt-1 text-gray-600 dark:text-gray-400">
                    {t("proxy.timeouts.first_byte_effective", {
                        text: firstByteText,
                        image: firstByteImage,
                        video: firstByteVideo,
                        aux: firstByteAux,
                        cap: TTFT_BASE_CAP,
                        defaultValue: `text ${firstByteText}s / image ${firstByteImage}s / video ${firstByteVideo}s / aux ${firstByteAux}s`,
                    })}
                </div>
            </div>

            {/* 底部操作栏 */}
            <div className="flex items-center justify-between pt-2 border-t border-gray-200/60 dark:border-base-300/60">
                <button
                    type="button"
                    onClick={handleResetDefaults}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-300 rounded-lg transition-colors cursor-pointer"
                >
                    <RotateCcw size={13} />
                    {t("proxy.upload_timeouts.btn_reset", { defaultValue: "恢复默认值" })}
                </button>

                {onSave && (
                    <button
                        type="button"
                        onClick={handleSave}
                        disabled={isSaving}
                        className={`inline-flex items-center gap-1.5 px-3.5 py-1.5 text-xs font-medium text-white rounded-lg transition-all cursor-pointer ${
                            savedSuccessfully ? "bg-emerald-600 hover:bg-emerald-700" : "bg-blue-600 hover:bg-blue-700 shadow-xs"
                        } disabled:opacity-50`}
                    >
                        {savedSuccessfully ? (
                            <>
                                <Check size={13} />
                                {t("common.saved", { defaultValue: "已保存" })}
                            </>
                        ) : (
                            <>
                                <Save size={13} />
                                {isSaving
                                    ? t("common.saving", { defaultValue: "保存中..." })
                                    : t("proxy.upload_timeouts.btn_save", { defaultValue: "保存超时设置" })}
                            </>
                        )}
                    </button>
                )}
            </div>
        </div>
    );
};

export default TimeoutSettings;

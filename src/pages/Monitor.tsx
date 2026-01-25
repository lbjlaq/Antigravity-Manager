import React from 'react';
import { ProxyMonitor } from '../components/proxy/ProxyMonitor';

const Monitor: React.FC = () => {
    return (
        <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
            <ProxyMonitor />
        </div>
    );
};

export default Monitor;